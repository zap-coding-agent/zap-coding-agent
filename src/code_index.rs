/// Tree-sitter + SQLite code index.
///
/// Indexes symbols (functions, structs, classes, methods, etc.) from source
/// files and stores them in a persistent SQLite database at `.zap/code.db`.
/// Supports incremental re-indexing based on file mtime.
///
/// Supported languages: Rust, Python, JavaScript, TypeScript, Go, Java.
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::UNIX_EPOCH;

// ── Public types ──────────────────────────────────────────────────────────────

/// Output of `CodeIndex::quality_report()` — used by `/index quality`.
#[derive(Debug, Default)]
pub struct QualityReport {
    pub total_files:     usize,
    pub total_syms:      usize,
    /// (impl_label, method_count, example_path)
    pub god_objects:     Vec<(String, usize, String)>,
    /// (path, symbol_count)
    pub large_files:     Vec<(String, usize)>,
    /// (fn_name, path, line, ref_count)
    pub high_coupling:   Vec<(String, String, usize, usize)>,
    /// (fn_name, path, line) — pub fn with ≤1 reference (likely dead code)
    pub dead_candidates: Vec<(String, String, usize)>,
    /// (fn_name, path, line) — signature was truncated (very long = complex)
    pub complex_fns:     Vec<(String, String, usize)>,
    /// (path, total_fns, async_fns)
    pub async_files:     Vec<(String, usize, usize)>,
}

impl QualityReport {
    /// 0-100 score. Deducts for structural issues.
    pub fn score(&self) -> u32 {
        let mut s = 100i32;
        for (_, methods, _) in &self.god_objects {
            s -= if *methods > 40 { 15 } else if *methods > 25 { 10 } else { 5 };
        }
        for (_, syms) in &self.large_files {
            s -= if *syms > 100 { 8 } else { 4 };
        }
        s -= (self.dead_candidates.len() as i32).min(15);
        s -= (self.complex_fns.len() as i32 * 2).min(10);
        s.max(0) as u32
    }
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub line: usize,
    pub signature: String,
    pub language: String,
    pub context: String,
}

impl Symbol {
    pub fn display(&self) -> String {
        if self.context.is_empty() {
            format!("{}:{} {} {} — {}", self.path, self.line, self.kind, self.name, self.signature)
        } else {
            format!("{}:{} {} {} [{}] — {}", self.path, self.line, self.kind, self.name, self.context, self.signature)
        }
    }
}

// ── Global singleton ──────────────────────────────────────────────────────────

static GLOBAL_INDEX: OnceLock<Arc<Mutex<CodeIndex>>> = OnceLock::new();

pub fn set_global(index: Arc<Mutex<CodeIndex>>) {
    let _ = GLOBAL_INDEX.set(index);
}

pub fn global_find_definition(name: &str) -> Vec<Symbol> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.find_definition(name).ok())
        .unwrap_or_default()
}

pub fn global_symbols_in_path(path: &str) -> Vec<Symbol> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.symbols_in_path(path).ok())
        .unwrap_or_default()
}

pub fn global_search(query: &str, limit: usize) -> Vec<Symbol> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.search(query, limit).ok())
        .unwrap_or_default()
}

pub fn global_list_indexed_files(limit: usize) -> Vec<(String, usize)> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.list_indexed_files(limit).ok())
        .unwrap_or_default()
}

pub fn global_stats() -> (usize, usize) {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.total_stats().ok())
        .unwrap_or((0, 0))
}

pub fn global_stats_by_kind() -> Vec<(String, usize)> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.stats_by_kind().ok())
        .unwrap_or_default()
}

pub fn global_top_files(n: usize) -> Vec<(String, usize)> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.top_files(n).ok())
        .unwrap_or_default()
}

pub fn global_quality_report() -> Option<QualityReport> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.quality_report().ok())
}

/// Returns (short_path, symbol_count, line_count) for all indexed files, sorted by line_count desc.
/// line_count is read from disk; files that no longer exist are skipped.
pub fn global_file_line_counts() -> Vec<(String, usize, usize)> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let files = global_list_indexed_files(500);
    let mut result: Vec<(String, usize, usize)> = files
        .into_iter()
        .filter_map(|(path, sym_count)| {
            let line_count = std::fs::read_to_string(&path)
                .map(|s| s.lines().count())
                .ok()?;
            let short = path
                .strip_prefix(cwd.to_str().unwrap_or(""))
                .unwrap_or(&path)
                .trim_start_matches('/')
                .to_string();
            Some((short, sym_count, line_count))
        })
        .collect();
    result.sort_by(|a, b| b.2.cmp(&a.2));
    result
}

pub fn global_compute_reference_counts() -> usize {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|mut g| g.compute_reference_counts().ok())
        .unwrap_or(0)
}

pub fn global_reindex_file(path: &Path) {
    if let Some(g) = GLOBAL_INDEX.get() {
        if let Ok(mut guard) = g.lock() {
            match guard.index_file(path) {
                Ok(n) => {
                    crate::log::write(
                        "INDEX",
                        &format!("tree-sitter · reindex · {} · {} symbols", path.display(), n),
                    );
                    // Keep ref_counts current so quality report and dead-code detection
                    // reflect the just-written file immediately.
                    let _ = guard.compute_reference_counts();
                }
                Err(e) => crate::log::write(
                    "WARN ",
                    &format!("tree-sitter · reindex failed · {}: {}", path.display(), e),
                ),
            }
        }
    }
}

/// Spawn a background tokio task that periodically re-indexes changed files.
/// Interval: `ZAP_INDEX_INTERVAL` env var (seconds), default 120.
/// Only logs to `zap.log` — never writes to stdout/TUI to avoid interrupting the user.
pub fn spawn_background_indexer(cwd: PathBuf) {
    tokio::spawn(async move {
        let secs = std::env::var("ZAP_INDEX_INTERVAL")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(120);
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(secs));
        interval.tick().await; // skip the immediate first tick
        loop {
            interval.tick().await;
            let Some(g) = GLOBAL_INDEX.get() else { continue };
            // try_lock so we don't block a foreground tool-use reindex
            let Ok(mut guard) = g.try_lock() else { continue };
            match guard.index_dir(&cwd) {
                Ok((files, symbols)) if files > 0 => {
                    crate::log::write(
                        "INDEX",
                        &format!("tree-sitter · background · {} files updated · {} symbols", files, symbols),
                    );
                }
                Ok(_) => {} // nothing changed — skip log noise
                Err(e) => crate::log::write("WARN ", &format!("background index error: {}", e)),
            }
        }
    });
}

// ── CodeIndex ─────────────────────────────────────────────────────────────────

pub struct CodeIndex {
    conn: Connection,
    db_path: PathBuf,
}

impl CodeIndex {
    pub fn open(project_root: &Path) -> Result<Self> {
        let dir = project_root.join(".zap");
        std::fs::create_dir_all(&dir)?;
        let db_path = dir.join("code.db");
        let conn = Connection::open(&db_path)
            .context("open code index db")?;

        // WAL mode for better concurrent read performance.
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

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

        // Migrations: add columns that may not exist in older DBs.
        let _ = conn.execute("ALTER TABLE symbols ADD COLUMN ref_count INTEGER DEFAULT 0", []);

        Ok(Self { conn, db_path })
    }

    pub fn db_path(&self) -> &Path { &self.db_path }

    /// True if the file has been modified since it was last indexed.
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
            .unwrap_or(true) // not in index → needs indexing
    }

    /// Index a single file. Replaces all existing symbols for this file.
    pub fn index_file(&mut self, path: &Path) -> Result<usize> {
        let source = std::fs::read_to_string(path)
            .context("read source file")?;
        let lang = detect_language(path);
        let path_str = path.to_string_lossy().to_string();
        let mtime = file_mtime(path).unwrap_or(0);

        let symbols = if lang.is_empty() {
            vec![]
        } else {
            extract_symbols(&source, &lang, &path_str)
        };

        // Delete old symbols and re-insert.
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

    /// Walk `dir`, indexing all source files that need reindexing.
    /// Returns `(files_indexed, symbols_added)`.
    pub fn index_dir(&mut self, dir: &Path) -> Result<(usize, usize)> {
        let mut files = 0;
        let mut symbols = 0;

        let extensions = &["rs", "py", "js", "ts", "tsx", "jsx", "go", "java"];
        let entries = walkdir_filtered(dir, extensions);

        for path in entries {
            if self.needs_reindex(&path) {
                match self.index_file(&path) {
                    Ok(n) => { files += 1; symbols += n; }
                    Err(e) => crate::log::write("WARN ", &format!("index: skip {:?}: {}", path, e)),
                }
            }
        }

        if files > 0 {
            crate::log::write(
                "INDEX",
                &format!("tree-sitter · scan complete · {} files · {} symbols · {}", files, symbols, dir.display()),
            );
            // Refresh reference counts after every reindex so quality data stays current.
            let _ = self.compute_reference_counts();
        }

        Ok((files, symbols))
    }

    /// Find definitions of a symbol by exact name (case-insensitive), then prefix.
    pub fn find_definition(&self, name: &str) -> Result<Vec<Symbol>> {
        let exact: Vec<Symbol> = self.conn
            .prepare("SELECT path, name, kind, line, signature, language, context
                       FROM symbols WHERE name = ?1 COLLATE NOCASE LIMIT 20")?
            .query_map(params![name], row_to_symbol)?
            .flatten()
            .collect();
        if !exact.is_empty() { return Ok(exact); }

        // Prefix fallback
        let pattern = format!("{}%", name);
        let results: Vec<Symbol> = self.conn
            .prepare("SELECT path, name, kind, line, signature, language, context
                       FROM symbols WHERE name LIKE ?1 LIMIT 20")?
            .query_map(params![pattern], row_to_symbol)?
            .flatten()
            .collect();
        Ok(results)
    }

    /// All symbols in a file or directory.
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

    /// Full-text search across name + signature.
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

    /// Returns all indexed files sorted by symbol count descending.
    pub fn list_indexed_files(&self, limit: usize) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, symbol_count FROM indexed_files ORDER BY symbol_count DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))?
            .flatten()
            .collect();
        Ok(rows)
    }

    /// Returns (indexed_files, total_symbols).
    pub fn total_stats(&self) -> Result<(usize, usize)> {
        let files: i64 = self.conn
            .query_row("SELECT COUNT(*) FROM indexed_files", [], |r| r.get(0))
            .unwrap_or(0);
        let syms: i64 = self.conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
            .unwrap_or(0);
        Ok((files as usize, syms as usize))
    }

    /// Count symbols per kind (fn, struct, enum, …), sorted by count descending.
    pub fn stats_by_kind(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT kind, COUNT(*) as n FROM symbols GROUP BY kind ORDER BY n DESC"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?.flatten().collect();
        Ok(rows)
    }

    /// Top N files by symbol count.
    pub fn top_files(&self, n: usize) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, symbol_count FROM indexed_files ORDER BY symbol_count DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map([n as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?.flatten().collect();
        Ok(rows)
    }

    /// Count symbols per language, sorted by count descending.
    pub fn stats_by_language(&self) -> Result<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare(
            "SELECT language, COUNT(*) as n FROM symbols WHERE language != '' GROUP BY language ORDER BY n DESC"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?.flatten().collect();
        Ok(rows)
    }

    /// Compute reference counts for all symbols by scanning source file text.
    ///
    /// Counts occurrences of `identifier(` patterns only — actual call sites.
    /// This avoids false positives from field access (`self.name`), variable uses,
    /// string literals, and comments that plagued the old word-frequency approach.
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

    /// Structured quality report for `/index quality`.
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

        // High coupling: unique functions only (COUNT=1 excludes trait methods that share
        // a name across many impls), minimum name length 5 filters generic words like
        // "name"/"path"/"id" whose word-frequency counts are meaningless.
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

    /// Remove entries for files that no longer exist.
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

// ── Call-site counter ─────────────────────────────────────────────────────────

/// Scan `source` and increment `freq[name]` for each `name(` occurrence.
///
/// Skips string literals and line comments so that identifiers inside
/// `"name("` or `// name(` don't inflate the counts.
fn count_call_sites(source: &str, freq: &mut std::collections::HashMap<String, usize>) {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    while i < len {
        let b = bytes[i];
        // Skip line comments (`//`)
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            while i < len && bytes[i] != b'\n' { i += 1; }
            continue;
        }
        // Skip block comments (`/* … */`)
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') { i += 1; }
            i += 2;
            continue;
        }
        // Skip string literals (`"…"` and `'…'`), respecting `\` escapes
        if b == b'"' || b == b'\'' {
            let q = b;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' { i += 2; continue; }
                if bytes[i] == q    { i += 1; break; }
                i += 1;
            }
            continue;
        }
        // Identifier start
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
            // Only count if immediately followed by `(` — an actual call site
            if i < len && bytes[i] == b'(' {
                let ident = &source[start..i];
                if ident.len() >= 2 {
                    *freq.entry(ident.to_string()).or_insert(0) += 1;
                }
            }
            continue;
        }
        i += 1;
    }
}

// ── Language detection ────────────────────────────────────────────────────────

fn detect_language(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs")       => "rust",
        Some("py")       => "python",
        Some("js") | Some("jsx")  => "javascript",
        Some("ts") | Some("tsx")  => "typescript",
        Some("go")       => "go",
        Some("java")     => "java",
        _ => "",
    }
}

// ── Walkdir helper ────────────────────────────────────────────────────────────

fn walkdir_filtered(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                // Skip hidden dirs and common non-code dirs.
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with('.') || matches!(name, "node_modules" | "target" | "vendor" | "__pycache__" | "dist" | "build") {
                    continue;
                }
                out.extend(walkdir_filtered(&p, exts));
            } else if p.extension().and_then(|e| e.to_str()).map(|e| exts.contains(&e)).unwrap_or(false) {
                out.push(p);
            }
        }
    }
    out
}

fn file_mtime(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

fn row_to_symbol(row: &rusqlite::Row) -> rusqlite::Result<Symbol> {
    Ok(Symbol {
        path:      row.get(0)?,
        name:      row.get(1)?,
        kind:      row.get(2)?,
        line:      row.get::<_, i64>(3)? as usize,
        signature: row.get(4)?,
        language:  row.get(5)?,
        context:   row.get(6)?,
    })
}

// ── Symbol extraction via tree-sitter ────────────────────────────────────────

struct RawSymbol {
    name: String,
    kind: String,
    line: usize,
    signature: String,
    context: String,
}

fn extract_symbols(source: &str, lang: &str, path: &str) -> Vec<Symbol> {
    let raw = match lang {
        "rust"       => extract_rust(source),
        "python"     => extract_python(source),
        "javascript" => extract_js(source, false),
        "typescript" => extract_js(source, true),
        "go"         => extract_go(source),
        "java"       => extract_java(source),
        _            => vec![],
    };

    raw.into_iter().map(|r| Symbol {
        path:      path.to_string(),
        name:      r.name,
        kind:      r.kind,
        line:      r.line,
        signature: r.signature,
        language:  lang.to_string(),
        context:   r.context,
    }).collect()
}

fn make_parser(language: tree_sitter::Language) -> Option<tree_sitter::Parser> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(language).ok()?;
    Some(parser)
}

// ── Signature helper ──────────────────────────────────────────────────────────

/// Extract a single-line signature for a node: text from node start up to the
/// first block/body child (exclusive). Capped at 120 chars.
fn signature(node: tree_sitter::Node, source: &[u8]) -> String {
    // Find the start of the body block if present.
    let body_start = body_start(node);
    let end = body_start.unwrap_or(node.end_byte());
    let text = &source[node.start_byte()..end.min(source.len())];
    let s = std::str::from_utf8(text).unwrap_or("").trim();
    // Collapse whitespace and trim.
    let collapsed: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 200 {
        let truncated: String = collapsed.chars().take(200).collect();
        format!("{}…", truncated)
    } else {
        collapsed
    }
}

/// Return the byte offset of the first body/block child of `node`.
fn body_start(node: tree_sitter::Node) -> Option<usize> {
    let body_kinds = &[
        "block", "statement_block", "suite", "declaration_list",
        "class_body", "enum_body", "field_declaration_list", "interface_body",
        "struct_body",
    ];
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if body_kinds.contains(&child.kind()) {
                return Some(child.start_byte());
            }
        }
    }
    None
}

fn node_text<'a>(node: tree_sitter::Node, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()]).unwrap_or("")
}

// ── Rust ──────────────────────────────────────────────────────────────────────

fn extract_rust(source: &str) -> Vec<RawSymbol> {
    let mut parser = match make_parser(tree_sitter_rust::language()) {
        Some(p) => p,
        None    => return vec![],
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None    => return vec![],
    };
    let mut out = Vec::new();
    extract_rust_node(tree.root_node(), source.as_bytes(), &mut out, "");
    out
}

fn extract_rust_node(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
    let kind = node.kind();
    match kind {
        "function_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let sig  = signature(node, src);
                out.push(RawSymbol { name: name.clone(), kind: "fn".into(), line: node.start_position().row + 1, signature: sig, context: context.to_string() });
                // Recurse into the body to find nested fns.
                let new_ctx = format!("fn {}", name);
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "block" {
                            extract_rust_node(c, src, out, &new_ctx);
                        }
                    }
                }
                return;
            }
        }
        "struct_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "struct".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "enum_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "enum".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "trait_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let ctx = format!("trait {}", name);
                out.push(RawSymbol { name: node_text(n, src).to_string(), kind: "trait".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        extract_rust_node(c, src, out, &ctx);
                    }
                }
                return;
            }
        }
        "impl_item" => {
            // Build impl context label: "impl Foo" or "impl Trait for Foo".
            let impl_label = build_impl_label(node, src);
            for i in 0..node.child_count() {
                if let Some(c) = node.child(i) {
                    extract_rust_node(c, src, out, &impl_label);
                }
            }
            return;
        }
        "const_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "const".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "type_item" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "type".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "macro_definition" => {
            // macro_rules! foo { ... }
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "macro".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            extract_rust_node(c, src, out, context);
        }
    }
}

fn build_impl_label(node: tree_sitter::Node, src: &[u8]) -> String {
    // impl Foo or impl Trait for Foo
    let mut parts = vec!["impl".to_string()];
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            match c.kind() {
                "for" => { parts.push("for".into()); }
                "type_identifier" | "generic_type" | "scoped_type_identifier" => {
                    parts.push(node_text(c, src).to_string());
                }
                "declaration_list" | "where_clause" => break,
                _ => {}
            }
        }
    }
    parts.join(" ")
}

// ── Python ────────────────────────────────────────────────────────────────────

fn extract_python(source: &str) -> Vec<RawSymbol> {
    let mut parser = match make_parser(tree_sitter_python::language()) {
        Some(p) => p,
        None    => return vec![],
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None    => return vec![],
    };
    let mut out = Vec::new();
    extract_python_node(tree.root_node(), source.as_bytes(), &mut out, "");
    out
}

fn extract_python_node(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
    let kind = node.kind();
    match kind {
        "function_definition" | "async_function_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let k = if kind == "async_function_definition" { "async fn" } else { "def" };
                out.push(RawSymbol { name: name.clone(), kind: k.into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                // Recurse into the body.
                let new_ctx = if context.is_empty() { name.clone() } else { format!("{}.{}", context, name) };
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "block" {
                            extract_python_node(c, src, out, &new_ctx);
                        }
                    }
                }
                return;
            }
        }
        "class_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "class".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = if context.is_empty() { name.clone() } else { format!("{}.{}", context, name) };
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        extract_python_node(c, src, out, &new_ctx);
                    }
                }
                return;
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            extract_python_node(c, src, out, context);
        }
    }
}

// ── JavaScript / TypeScript ───────────────────────────────────────────────────

fn extract_js(source: &str, typescript: bool) -> Vec<RawSymbol> {
    let lang = if typescript {
        tree_sitter_typescript::language_typescript()
    } else {
        tree_sitter_javascript::language()
    };
    let mut parser = match make_parser(lang) {
        Some(p) => p,
        None    => return vec![],
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None    => return vec![],
    };
    let mut out = Vec::new();
    extract_js_node(tree.root_node(), source.as_bytes(), &mut out, "");
    out
}

fn extract_js_node(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
    let kind = node.kind();
    match kind {
        "function_declaration" | "generator_function_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let k = if kind == "generator_function_declaration" { "function*" } else { "function" };
                out.push(RawSymbol { name: name.clone(), kind: k.into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "statement_block" {
                            extract_js_node(c, src, out, &new_ctx);
                        }
                    }
                }
                return;
            }
        }
        "class_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "class".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        extract_js_node(c, src, out, &new_ctx);
                    }
                }
                return;
            }
        }
        "method_definition" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "method".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                return;
            }
        }
        // TypeScript-specific
        "interface_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "interface".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        extract_js_node(c, src, out, &new_ctx);
                    }
                }
                return;
            }
        }
        "type_alias_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "type".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        "enum_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "enum".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
            }
        }
        // const/let/var with function/arrow right-hand side
        "lexical_declaration" | "variable_declaration" => {
            extract_js_var_decls(node, src, out, context);
            return;
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            extract_js_node(c, src, out, context);
        }
    }
}

fn extract_js_var_decls(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
    for i in 0..node.child_count() {
        if let Some(decl) = node.child(i) {
            if decl.kind() == "variable_declarator" {
                if let (Some(name_node), Some(val_node)) = (
                    decl.child_by_field_name("name"),
                    decl.child_by_field_name("value"),
                ) {
                    let val_kind = val_node.kind();
                    if matches!(val_kind, "arrow_function" | "function" | "generator_function") {
                        let name = node_text(name_node, src).to_string();
                        let k = if val_kind == "arrow_function" { "arrow fn" } else { "function" };
                        out.push(RawSymbol {
                            name,
                            kind: k.into(),
                            line: decl.start_position().row + 1,
                            signature: signature(decl, src),
                            context: context.to_string(),
                        });
                    }
                }
            }
        }
    }
}

// ── Go ────────────────────────────────────────────────────────────────────────

fn extract_go(source: &str) -> Vec<RawSymbol> {
    let mut parser = match make_parser(tree_sitter_go::language()) {
        Some(p) => p,
        None    => return vec![],
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None    => return vec![],
    };
    let mut out = Vec::new();
    extract_go_node(tree.root_node(), source.as_bytes(), &mut out, "");
    out
}

fn extract_go_node(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
    let kind = node.kind();
    match kind {
        "function_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "func".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                return;
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let recv = node.child_by_field_name("receiver")
                    .map(|r| node_text(r, src).trim_matches(|c| c == '(' || c == ')').trim().to_string())
                    .unwrap_or_default();
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "method".into(), line: node.start_position().row + 1, signature: signature(node, src), context: recv });
                return;
            }
        }
        "type_declaration" => {
            // type Foo struct { ... } — contains a type_spec child.
            for i in 0..node.child_count() {
                if let Some(spec) = node.child(i) {
                    if spec.kind() == "type_spec" {
                        if let Some(n) = spec.child_by_field_name("name") {
                            let name = node_text(n, src).to_string();
                            let type_kind = spec.child_by_field_name("type")
                                .map(|t| t.kind())
                                .unwrap_or("type");
                            let k = match type_kind {
                                "struct_type"    => "struct",
                                "interface_type" => "interface",
                                _                => "type",
                            };
                            out.push(RawSymbol { name, kind: k.into(), line: spec.start_position().row + 1, signature: signature(spec, src), context: context.to_string() });
                        }
                    }
                }
            }
            return;
        }
        "const_declaration" | "var_declaration" => {
            let k = if kind == "const_declaration" { "const" } else { "var" };
            for i in 0..node.child_count() {
                if let Some(spec) = node.child(i) {
                    if matches!(spec.kind(), "const_spec" | "var_spec") {
                        if let Some(n) = spec.child_by_field_name("name") {
                            let name = node_text(n, src).to_string();
                            out.push(RawSymbol { name, kind: k.into(), line: spec.start_position().row + 1, signature: signature(spec, src), context: context.to_string() });
                        }
                    }
                }
            }
            return;
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            extract_go_node(c, src, out, context);
        }
    }
}

// ── Java ──────────────────────────────────────────────────────────────────────

fn extract_java(source: &str) -> Vec<RawSymbol> {
    let mut parser = match make_parser(tree_sitter_java::language()) {
        Some(p) => p,
        None    => return vec![],
    };
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None    => return vec![],
    };
    let mut out = Vec::new();
    extract_java_node(tree.root_node(), source.as_bytes(), &mut out, "");
    out
}

fn extract_java_node(node: tree_sitter::Node, src: &[u8], out: &mut Vec<RawSymbol>, context: &str) {
    let kind = node.kind();
    match kind {
        "class_declaration" | "enum_declaration" | "record_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                let k = match kind {
                    "enum_declaration"   => "enum",
                    "record_declaration" => "record",
                    _                    => "class",
                };
                out.push(RawSymbol { name: name.clone(), kind: k.into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        extract_java_node(c, src, out, &new_ctx);
                    }
                }
                return;
            }
        }
        "interface_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name: name.clone(), kind: "interface".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                let new_ctx = name.clone();
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        extract_java_node(c, src, out, &new_ctx);
                    }
                }
                return;
            }
        }
        "method_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "method".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                return;
            }
        }
        "constructor_declaration" => {
            if let Some(n) = node.child_by_field_name("name") {
                let name = node_text(n, src).to_string();
                out.push(RawSymbol { name, kind: "constructor".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                return;
            }
        }
        "field_declaration" => {
            // Only top-level constants (public static final).
            let text = node_text(node, src);
            if text.contains("static") && text.contains("final") {
                // Extract the variable declarator names.
                for i in 0..node.child_count() {
                    if let Some(c) = node.child(i) {
                        if c.kind() == "variable_declarator" {
                            if let Some(n) = c.child_by_field_name("name") {
                                let name = node_text(n, src).to_string();
                                out.push(RawSymbol { name, kind: "const".into(), line: node.start_position().row + 1, signature: signature(node, src), context: context.to_string() });
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    for i in 0..node.child_count() {
        if let Some(c) = node.child(i) {
            extract_java_node(c, src, out, context);
        }
    }
}
