use anyhow::{Context, Result};
use rusqlite::params;
use std::path::Path;

use super::{graph_enabled, CodeIndex};
use super::extract::extract_all;
use super::walk::{detect_language, file_mtime, walkdir_filtered};

impl CodeIndex {
    pub fn open(project_root: &Path) -> Result<Self> {
        let dir = project_root.join(".zap");
        std::fs::create_dir_all(&dir)?;
        let db_path = dir.join("code.db");
        let conn = rusqlite::Connection::open(&db_path)
            .context("open code index db")?;

        // WAL mode is faster but can fail on Windows with locked .db-shm files
        // (antivirus, OneDrive, network drives). Fall back to DELETE silently.
        conn.execute_batch("PRAGMA synchronous=NORMAL;")
            .context("set synchronous pragma")?;
        if conn.execute_batch("PRAGMA journal_mode=WAL;").is_err() {
            let _ = conn.execute_batch("PRAGMA journal_mode=DELETE;");
        }

        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS symbols (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                path      TEXT NOT NULL,
                name      TEXT NOT NULL,
                kind      TEXT NOT NULL,
                line      INTEGER NOT NULL,
                signature TEXT NOT NULL DEFAULT '',
                language  TEXT NOT NULL DEFAULT '',
                context   TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_sym_name ON symbols(name COLLATE NOCASE);
            CREATE INDEX IF NOT EXISTS idx_sym_path ON symbols(path);

            CREATE TABLE IF NOT EXISTS indexed_files (
                path          TEXT PRIMARY KEY,
                mtime         INTEGER NOT NULL,
                symbol_count  INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS call_sites (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                path          TEXT    NOT NULL,
                line          INTEGER NOT NULL,
                col           INTEGER NOT NULL DEFAULT 0,
                name          TEXT    NOT NULL,
                qualifier     TEXT    NOT NULL DEFAULT '',
                receiver_expr TEXT    NOT NULL DEFAULT '',
                caller_scope  TEXT    NOT NULL DEFAULT '',
                language      TEXT    NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_cs_name      ON call_sites(name COLLATE NOCASE);
            CREATE INDEX IF NOT EXISTS idx_cs_path      ON call_sites(path);
            CREATE INDEX IF NOT EXISTS idx_cs_qualifier ON call_sites(qualifier);

            CREATE TABLE IF NOT EXISTS imports (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                path           TEXT    NOT NULL,
                line           INTEGER NOT NULL,
                module         TEXT    NOT NULL,
                imported_name  TEXT    NOT NULL DEFAULT '',
                alias          TEXT    NOT NULL DEFAULT '',
                language       TEXT    NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_imp_path ON imports(path);
            CREATE INDEX IF NOT EXISTS idx_imp_name ON imports(imported_name COLLATE NOCASE);
            CREATE INDEX IF NOT EXISTS idx_imp_mod  ON imports(module);

            CREATE TABLE IF NOT EXISTS file_rank (
                path TEXT PRIMARY KEY,
                rank REAL NOT NULL DEFAULT 0
            );
        ")?;

        let _ = conn.execute("ALTER TABLE symbols ADD COLUMN ref_count INTEGER DEFAULT 0", []);

        Ok(Self { conn, db_path })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = rusqlite::Connection::open_in_memory().context("open in-memory code index")?;
        conn.execute_batch("
            CREATE TABLE IF NOT EXISTS symbols (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                path      TEXT NOT NULL,
                name      TEXT NOT NULL,
                kind      TEXT NOT NULL,
                line      INTEGER NOT NULL,
                signature TEXT NOT NULL DEFAULT '',
                language  TEXT NOT NULL DEFAULT '',
                context   TEXT NOT NULL DEFAULT '',
                ref_count INTEGER DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_sym_name ON symbols(name COLLATE NOCASE);
            CREATE INDEX IF NOT EXISTS idx_sym_path ON symbols(path);
            CREATE TABLE IF NOT EXISTS indexed_files (
                path          TEXT PRIMARY KEY,
                mtime         INTEGER NOT NULL,
                symbol_count  INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS call_sites (
                id            INTEGER PRIMARY KEY AUTOINCREMENT,
                path          TEXT    NOT NULL,
                line          INTEGER NOT NULL,
                col           INTEGER NOT NULL DEFAULT 0,
                name          TEXT    NOT NULL,
                qualifier     TEXT    NOT NULL DEFAULT '',
                receiver_expr TEXT    NOT NULL DEFAULT '',
                caller_scope  TEXT    NOT NULL DEFAULT '',
                language      TEXT    NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_cs_name      ON call_sites(name COLLATE NOCASE);
            CREATE INDEX IF NOT EXISTS idx_cs_path      ON call_sites(path);
            CREATE INDEX IF NOT EXISTS idx_cs_qualifier ON call_sites(qualifier);
            CREATE TABLE IF NOT EXISTS imports (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                path           TEXT    NOT NULL,
                line           INTEGER NOT NULL,
                module         TEXT    NOT NULL,
                imported_name  TEXT    NOT NULL DEFAULT '',
                alias          TEXT    NOT NULL DEFAULT '',
                language       TEXT    NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_imp_path ON imports(path);
            CREATE INDEX IF NOT EXISTS idx_imp_name ON imports(imported_name COLLATE NOCASE);
            CREATE INDEX IF NOT EXISTS idx_imp_mod  ON imports(module);
            CREATE TABLE IF NOT EXISTS file_rank (
                path TEXT PRIMARY KEY,
                rank REAL NOT NULL DEFAULT 0
            );
        ")?;
        Ok(Self { conn, db_path: std::path::PathBuf::from(":memory:") })
    }

    pub fn db_path(&self) -> &Path { &self.db_path }

    pub fn is_in_memory(&self) -> bool {
        self.db_path == std::path::Path::new(":memory:")
    }

    pub fn needs_reindex(&self, path: &Path) -> bool {
        let mtime = file_mtime(path).unwrap_or(0);
        let path_str = path.to_string_lossy();
        self.conn
            .query_row(
                "SELECT mtime FROM indexed_files WHERE path = ?1",
                params![path_str],
                |row| row.get::<_, i64>(0),
            )
            .map(|stored| stored < mtime as i64)
            .unwrap_or(true)
    }

    pub fn index_file(&mut self, path: &Path) -> Result<usize> {
        let source = std::fs::read_to_string(path)
            .context("read source file")?;
        let lang = detect_language(path);
        let path_str = path.to_string_lossy().to_string();
        let mtime = file_mtime(path).unwrap_or(0);
        let graph = graph_enabled();

        let extracted = if lang.is_empty() {
            super::extract::ExtractResult { symbols: vec![], call_sites: vec![], imports: vec![] }
        } else {
            extract_all(&source, lang, &path_str)
        };

        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM symbols    WHERE path = ?1", params![path_str])?;
        tx.execute("DELETE FROM call_sites WHERE path = ?1", params![path_str])?;
        tx.execute("DELETE FROM imports    WHERE path = ?1", params![path_str])?;

        for sym in &extracted.symbols {
            tx.execute(
                "INSERT INTO symbols (path, name, kind, line, signature, language, context)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    sym.path, sym.name, sym.kind, sym.line as i64,
                    sym.signature, sym.language, sym.context
                ],
            )?;
        }

        if graph {
            for cs in &extracted.call_sites {
                tx.execute(
                    "INSERT INTO call_sites (path, line, col, name, qualifier, receiver_expr, caller_scope, language)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        cs.path, cs.line as i64, cs.col as i64,
                        cs.name, cs.qualifier, cs.receiver_expr, cs.caller_scope, cs.language
                    ],
                )?;
            }
            for im in &extracted.imports {
                tx.execute(
                    "INSERT INTO imports (path, line, module, imported_name, alias, language)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        im.path, im.line as i64,
                        im.module, im.imported_name, im.alias, im.language
                    ],
                )?;
            }
        }

        let count = extracted.symbols.len() as i64;
        tx.execute(
            "INSERT OR REPLACE INTO indexed_files (path, mtime, symbol_count)
             VALUES (?1, ?2, ?3)",
            params![path_str, mtime as i64, count],
        )?;
        tx.commit()?;

        if !lang.is_empty() {
            if graph {
                crate::log::write(
                    "INDEX",
                    &format!(
                        "tree-sitter · {} · {} · {} symbols · {} calls · {} imports",
                        lang, path_str,
                        extracted.symbols.len(),
                        extracted.call_sites.len(),
                        extracted.imports.len(),
                    ),
                );
            } else {
                crate::log::write(
                    "INDEX",
                    &format!("tree-sitter · {} · {} · {} symbols", lang, path_str, extracted.symbols.len()),
                );
            }
        }

        Ok(extracted.symbols.len())
    }

    pub fn index_dir(&mut self, dir: &Path) -> Result<(usize, usize)> {
        let mut files = 0;
        let mut symbols = 0;
        let mut skipped = 0usize;
        let mut first_err: Option<String> = None;

        let extensions = &["rs", "py", "js", "ts", "tsx", "jsx", "go", "java", "cs"];
        let entries = walkdir_filtered(dir, extensions);

        for path in entries {
            if self.needs_reindex(&path) {
                match self.index_file(&path) {
                    Ok(n) => { files += 1; symbols += n; }
                    Err(e) => {
                        let msg = e.to_string();
                        if first_err.is_none() { first_err = Some(msg.clone()); }
                        skipped += 1;
                        // Bail immediately on readonly — avoids holding the mutex for O(N)
                        // files and prevents the TUI from hanging on foreground tool calls.
                        if msg.contains("readonly") || msg.contains("read only") || msg.contains("ReadOnly") {
                            break;
                        }
                    }
                }
            }
        }

        if skipped > 0 {
            crate::log::write("WARN ", &format!(
                "index: {} file(s) skipped — {} (fix .zap/code.db permissions or run /init again)",
                skipped, first_err.as_deref().unwrap_or_default()
            ));
        }

        if files > 0 {
            crate::log::write(
                "INDEX",
                &format!("tree-sitter · scan complete · {} files · {} symbols · {}", files, symbols, dir.display()),
            );
            let _ = self.compute_reference_counts();
            if graph_enabled() {
                if let Ok(n) = self.compute_file_ranks() {
                    crate::log::write("INDEX", &format!("pagerank · {} files ranked", n));
                }
            }
        }

        // Propagate failure so the background indexer can count consecutive errors
        // and stop retrying after 3 — without this it loops forever every 120 s.
        if files == 0 && skipped > 0 {
            return Err(anyhow::anyhow!("{}", first_err.unwrap_or_else(|| "all files skipped".to_string())));
        }

        Ok((files, symbols))
    }

    pub fn list_indexed_files(&self, limit: usize) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, symbol_count FROM indexed_files ORDER BY symbol_count DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?.flatten().collect();
        Ok(rows)
    }

    pub fn total_stats(&self) -> Result<(usize, usize)> {
        let files: i64 = self.conn
            .query_row("SELECT COUNT(*) FROM indexed_files", [], |r| r.get(0))
            .unwrap_or(0);
        let syms: i64 = self.conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
            .unwrap_or(0);
        Ok((files as usize, syms as usize))
    }

    pub fn stats_by_kind(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, COUNT(*) as n FROM symbols GROUP BY kind ORDER BY n DESC"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?.flatten().collect();
        Ok(rows)
    }

    pub fn top_files(&self, n: usize) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, symbol_count FROM indexed_files ORDER BY symbol_count DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map([n as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?.flatten().collect();
        Ok(rows)
    }

    pub fn stats_by_language(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT language, COUNT(*) as n FROM symbols WHERE language != '' GROUP BY language ORDER BY n DESC"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?.flatten().collect();
        Ok(rows)
    }

    /// Refresh `symbols.ref_count` from the call_sites table.
    ///
    /// In graph mode this is a single SQL aggregate — name-based, case-insensitive,
    /// matches how `find_definition` resolves names. In symbols-only mode (the
    /// env-var fallback) call_sites is empty, so every ref_count goes to 0.
    /// That's the correct answer when graph data is disabled — callers who need
    /// the old text-scan behavior should set `ZAP_INDEX_MODE=graph` (default).
    pub fn compute_reference_counts(&mut self) -> Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        let updated = tx.execute(
            "UPDATE symbols
                SET ref_count = (
                    SELECT COUNT(*) FROM call_sites cs
                     WHERE cs.name = symbols.name COLLATE NOCASE
                )",
            [],
        )?;
        tx.commit()?;
        Ok(updated)
    }

    pub fn clear(&mut self) -> Result<()> {
        self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.conn.execute_batch("DELETE FROM symbols; DELETE FROM indexed_files;")?;
        Ok(())
    }

    pub fn prune_deleted(&mut self) -> Result<usize> {
        let paths: Vec<String> = self.conn
            .prepare("SELECT path FROM indexed_files")?
            .query_map([], |r| r.get(0))?
            .flatten()
            .collect();

        let mut removed = 0;
        for p in paths {
            if !Path::new(&p).exists() {
                self.conn.execute("DELETE FROM symbols WHERE path = ?1", params![p])?;
                self.conn.execute("DELETE FROM indexed_files WHERE path = ?1", params![p])?;
                removed += 1;
            }
        }
        Ok(removed)
    }
}
