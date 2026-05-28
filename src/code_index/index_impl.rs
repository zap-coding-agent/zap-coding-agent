use anyhow::{Context, Result};
use rusqlite::params;
use std::path::Path;

use super::{CodeIndex, QualityReport, Symbol};
use super::extract::extract_symbols;
use super::walk::{count_call_sites, detect_language, file_mtime, row_to_symbol, walkdir_filtered};

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
        ")?;
        Ok(Self { conn, db_path: std::path::PathBuf::from(":memory:") })
    }

    pub fn db_path(&self) -> &Path { &self.db_path }

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

        let symbols = if lang.is_empty() {
            vec![]
        } else {
            extract_symbols(&source, lang, &path_str)
        };

        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM symbols WHERE path = ?1", params![path_str])?;
        for sym in &symbols {
            tx.execute(
                "INSERT INTO symbols (path, name, kind, line, signature, language, context)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    sym.path, sym.name, sym.kind, sym.line as i64,
                    sym.signature, sym.language, sym.context
                ],
            )?;
        }
        let count = symbols.len() as i64;
        tx.execute(
            "INSERT OR REPLACE INTO indexed_files (path, mtime, symbol_count)
             VALUES (?1, ?2, ?3)",
            params![path_str, mtime as i64, count],
        )?;
        tx.commit()?;

        if !lang.is_empty() {
            crate::log::write(
                "INDEX",
                &format!("tree-sitter · {} · {} · {} symbols", lang, path_str, symbols.len()),
            );
        }

        Ok(symbols.len())
    }

    pub fn index_dir(&mut self, dir: &Path) -> Result<(usize, usize)> {
        let mut files = 0;
        let mut symbols = 0;
        let mut skipped = 0usize;
        let mut first_err: Option<String> = None;

        let extensions = &["rs", "py", "js", "ts", "tsx", "jsx", "go", "java"];
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
        }

        // Propagate failure so the background indexer can count consecutive errors
        // and stop retrying after 3 — without this it loops forever every 120 s.
        if files == 0 && skipped > 0 {
            return Err(anyhow::anyhow!("{}", first_err.unwrap_or_else(|| "all files skipped".to_string())));
        }

        Ok((files, symbols))
    }

    pub fn find_definition(&self, name: &str) -> Result<Vec<Symbol>> {
        let exact: Vec<Symbol> = self.conn
            .prepare("SELECT path, name, kind, line, signature, language, context
                       FROM symbols WHERE name = ?1 COLLATE NOCASE LIMIT 20")?
            .query_map(params![name], row_to_symbol)?
            .flatten()
            .collect();
        if !exact.is_empty() { return Ok(exact); }

        let pattern = format!("{}%", name);
        let results: Vec<Symbol> = self.conn
            .prepare("SELECT path, name, kind, line, signature, language, context
                       FROM symbols WHERE name LIKE ?1 LIMIT 20")?
            .query_map(params![pattern], row_to_symbol)?
            .flatten()
            .collect();
        Ok(results)
    }

    pub fn symbols_in_path(&self, path: &str) -> Result<Vec<Symbol>> {
        let pattern = if path.ends_with('/') || Path::new(path).is_dir() {
            format!("{}%", path.trim_end_matches('/'))
        } else {
            path.to_string()
        };

        let results: Vec<Symbol> = if pattern.contains('%') {
            self.conn
                .prepare("SELECT path, name, kind, line, signature, language, context
                           FROM symbols WHERE path LIKE ?1 ORDER BY path, line LIMIT 2000")?
                .query_map(params![pattern], row_to_symbol)?
                .flatten()
                .collect()
        } else {
            self.conn
                .prepare("SELECT path, name, kind, line, signature, language, context
                           FROM symbols WHERE path = ?1 ORDER BY line LIMIT 2000")?
                .query_map(params![pattern], row_to_symbol)?
                .flatten()
                .collect()
        };
        Ok(results)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Symbol>> {
        let pattern = format!("%{}%", query);
        let results: Vec<Symbol> = self.conn
            .prepare("SELECT path, name, kind, line, signature, language, context
                       FROM symbols
                       WHERE name LIKE ?1 OR signature LIKE ?1
                       ORDER BY CASE WHEN name LIKE ?2 THEN 0 ELSE 1 END, name
                       LIMIT ?3")?
            .query_map(params![pattern, format!("{}%", query), limit as i64], row_to_symbol)?
            .flatten()
            .collect();
        Ok(results)
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

    pub fn compute_reference_counts(&mut self) -> Result<usize> {
        let files: Vec<String> = self.conn
            .prepare("SELECT path FROM indexed_files")?
            .query_map([], |r| r.get(0))?
            .flatten()
            .collect();

        let mut call_freq: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for path in &files {
            if let Ok(content) = std::fs::read_to_string(path) {
                count_call_sites(&content, &mut call_freq);
            }
        }

        let symbols: Vec<(i64, String)> = self.conn
            .prepare("SELECT id, name FROM symbols")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .flatten()
            .collect();

        let tx = self.conn.unchecked_transaction()?;
        let mut updated = 0usize;
        for (id, name) in &symbols {
            let count = call_freq.get(name.as_str()).copied().unwrap_or(0) as i64;
            tx.execute("UPDATE symbols SET ref_count=?1 WHERE id=?2", params![count, id])?;
            updated += 1;
        }
        tx.commit()?;
        Ok(updated)
    }

    pub fn quality_report(&self) -> Result<QualityReport> {
        let (total_files, total_syms) = self.total_stats().unwrap_or((0, 0));

        let mut stmt = self.conn.prepare(
            "SELECT context, COUNT(*) as n, MAX(path) as path \
             FROM symbols WHERE kind='fn' AND context != '' \
             GROUP BY context HAVING n > 15 ORDER BY n DESC LIMIT 10"
        )?;
        let god_objects: Vec<(String, usize, String)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize, r.get::<_, String>(2)?))
        })?.flatten().collect();
        drop(stmt);

        let mut stmt = self.conn.prepare(
            "SELECT path, symbol_count FROM indexed_files WHERE symbol_count > 50 \
             ORDER BY symbol_count DESC LIMIT 8"
        )?;
        let large_files: Vec<(String, usize)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?.flatten().collect();
        drop(stmt);

        let mut stmt = self.conn.prepare(
            "SELECT name, MIN(path) as path, MIN(line) as line, ref_count \
             FROM symbols WHERE kind='fn' AND ref_count > 5 AND LENGTH(name) >= 5 \
             GROUP BY name HAVING COUNT(*) = 1 \
             ORDER BY ref_count DESC LIMIT 10"
        )?;
        let high_coupling: Vec<(String, String, usize, usize)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? as usize, r.get::<_, i64>(3)? as usize))
        })?.flatten().collect();
        drop(stmt);

        let mut stmt = self.conn.prepare(
            "SELECT name, path, line FROM symbols \
             WHERE kind='fn' AND signature LIKE 'pub %' \
             AND (ref_count = 0 OR ref_count = 1) \
             AND name NOT IN ('main','new','default','from','into','clone','fmt','drop') \
             ORDER BY path LIMIT 15"
        )?;
        let dead_candidates: Vec<(String, String, usize)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)? as usize))
        })?.flatten().collect();
        drop(stmt);

        let mut stmt = self.conn.prepare(
            "SELECT name, path, line FROM symbols \
             WHERE kind='fn' AND signature LIKE '%…' \
             ORDER BY LENGTH(signature) DESC LIMIT 10"
        )?;
        let complex_fns: Vec<(String, String, usize)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)? as usize))
        })?.flatten().collect();
        drop(stmt);

        let mut stmt = self.conn.prepare(
            "SELECT path, COUNT(*) as total, \
             SUM(CASE WHEN signature LIKE '%async%' THEN 1 ELSE 0 END) as async_n \
             FROM symbols WHERE kind='fn' \
             GROUP BY path HAVING total > 5 AND async_n > 0 \
             ORDER BY CAST(async_n AS REAL)/total DESC LIMIT 8"
        )?;
        let async_files: Vec<(String, usize, usize)> = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize, r.get::<_, i64>(2)? as usize))
        })?.flatten().collect();
        drop(stmt);

        Ok(QualityReport {
            total_files, total_syms,
            god_objects, large_files, high_coupling,
            dead_candidates, complex_fns, async_files,
        })
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
