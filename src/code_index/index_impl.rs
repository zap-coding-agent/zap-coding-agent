use anyhow::{Context, Result};
use rusqlite::params;
use std::path::Path;

use super::{graph_enabled, CallSite, CodeIndex, Import, PackedContext, PackedItem, QualityReport, Symbol};
use super::extract::extract_all;
use super::walk::{detect_language, file_mtime, row_to_call_site, row_to_import, row_to_symbol, walkdir_filtered};

/// Stopwords stripped from task text before keyword extraction. Very small list — code-relevant
/// terms like "fix", "render", "parser" are kept on purpose; only the most generic English glue is removed.
const TASK_STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "into", "that", "this", "these", "those",
    "what", "where", "when", "which", "how", "why", "who",
    "are", "was", "were", "been", "being",
    "has", "have", "had", "does", "did",
    "can", "should", "could", "would", "may", "might", "will",
    "you", "your", "they", "them",
];

fn task_keywords(task: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let push = |cur: &mut String, out: &mut Vec<String>| {
        if cur.len() >= 3 {
            let lower = cur.to_lowercase();
            if !TASK_STOPWORDS.contains(&lower.as_str()) && !out.contains(&lower) {
                out.push(lower);
            }
        }
        cur.clear();
    };
    for c in task.chars() {
        if c.is_ascii_alphanumeric() || c == '_' { cur.push(c); }
        else { push(&mut cur, &mut out); }
    }
    push(&mut cur, &mut out);
    out
}

fn approx_item_cost(it: &PackedItem) -> usize {
    // Roughly the display row length: path + line + name + signature + provenance + framing.
    it.path.len() + it.name.len() + it.signature.len() + it.provenance.len() + 24
}

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

    /// All call sites that reference `name` (case-insensitive). The headline graph query.
    /// Results are ordered by the caller file's PageRank when available — load-bearing
    /// files first — falling back to path/line order when ranks aren't computed yet.
    pub fn find_references(&self, name: &str, limit: usize) -> Result<Vec<CallSite>> {
        // Pull 2x the limit so the rank-based reorder has room to be useful before truncation.
        let raw: Vec<CallSite> = self.conn
            .prepare("SELECT path, line, col, name, qualifier, receiver_expr, caller_scope, language
                       FROM call_sites
                      WHERE name = ?1 COLLATE NOCASE
                      ORDER BY path, line
                      LIMIT ?2")?
            .query_map(params![name, (limit * 2).max(limit) as i64], row_to_call_site)?
            .flatten()
            .collect();

        // Annotate with rank, sort by rank desc, truncate.
        let mut with_rank: Vec<(f32, CallSite)> = raw.into_iter()
            .map(|cs| (self.file_rank(&cs.path), cs))
            .collect();
        with_rank.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.path.cmp(&b.1.path))
            .then_with(|| a.1.line.cmp(&b.1.line)));
        with_rank.truncate(limit);
        Ok(with_rank.into_iter().map(|(_, cs)| cs).collect())
    }

    /// References narrowed by qualifier — e.g. only `Bar::foo` instead of every `foo` call.
    /// Pass `qualifier=None` to match any qualifier; pass `Some("")` to match bare/unqualified calls only.
    pub fn callers_of(&self, name: &str, qualifier: Option<&str>, limit: usize) -> Result<Vec<CallSite>> {
        let results: Vec<CallSite> = match qualifier {
            None => self.find_references(name, limit)?,
            Some(q) => self.conn
                .prepare("SELECT path, line, col, name, qualifier, receiver_expr, caller_scope, language
                           FROM call_sites
                          WHERE name = ?1 COLLATE NOCASE AND qualifier = ?2
                          ORDER BY path, line
                          LIMIT ?3")?
                .query_map(params![name, q, limit as i64], row_to_call_site)?
                .flatten()
                .collect(),
        };
        Ok(results)
    }

    /// What does this file pull in.
    pub fn imports_for(&self, path: &str) -> Result<Vec<Import>> {
        let results: Vec<Import> = self.conn
            .prepare("SELECT path, line, module, imported_name, alias, language
                       FROM imports
                      WHERE path = ?1
                      ORDER BY line")?
            .query_map(params![path], row_to_import)?
            .flatten()
            .collect();
        Ok(results)
    }

    /// Which files import this name — blast radius for a rename.
    pub fn importers_of(&self, name: &str) -> Result<Vec<Import>> {
        let results: Vec<Import> = self.conn
            .prepare("SELECT path, line, module, imported_name, alias, language
                       FROM imports
                      WHERE imported_name = ?1 COLLATE NOCASE OR alias = ?1 COLLATE NOCASE
                      ORDER BY path, line")?
            .query_map(params![name], row_to_import)?
            .flatten()
            .collect();
        Ok(results)
    }

    /// Files that import from a specific module (e.g. `crate::util`, `react`).
    pub fn users_of_module(&self, module: &str) -> Result<Vec<Import>> {
        let pattern = format!("{}%", module);
        let results: Vec<Import> = self.conn
            .prepare("SELECT path, line, module, imported_name, alias, language
                       FROM imports
                      WHERE module = ?1 OR module LIKE ?2
                      ORDER BY path, line")?
            .query_map(params![module, pattern], row_to_import)?
            .flatten()
            .collect();
        Ok(results)
    }

    /// Rebuild `file_rank` from the call_sites + imports graph using PageRank.
    ///
    /// Edges are resolved by name: every call_site (caller_path, callee_name) becomes
    /// fractional edges to all files defining a symbol with that name. Same for imports.
    /// Damping = 0.85, iterations = 25, classic PageRank. Pure in-memory; no extra deps.
    pub fn compute_file_ranks(&mut self) -> Result<usize> {
        // 1. Collect all indexed files (these are the nodes).
        let files: Vec<String> = self.conn
            .prepare("SELECT path FROM indexed_files")?
            .query_map([], |r| r.get(0))?
            .flatten()
            .collect();
        if files.is_empty() { return Ok(0); }

        let mut file_index: std::collections::HashMap<String, usize> =
            std::collections::HashMap::with_capacity(files.len());
        for (i, p) in files.iter().enumerate() {
            file_index.insert(p.clone(), i);
        }

        // 2. Build name → set<defining_file_idx>.
        // Symbols are lowercased for case-insensitive resolution.
        let mut defs_by_name: std::collections::HashMap<String, Vec<usize>> =
            std::collections::HashMap::new();
        {
            let mut stmt = self.conn.prepare("SELECT name, path FROM symbols")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows.flatten() {
                if let Some(&fi) = file_index.get(&row.1) {
                    let key = row.0.to_lowercase();
                    let entry = defs_by_name.entry(key).or_default();
                    if !entry.contains(&fi) { entry.push(fi); }
                }
            }
        }

        // 3. Build edge weights per (caller_idx, callee_idx).
        // Using a flat HashMap keyed on packed (u32, u32) for compactness.
        let mut edges: std::collections::HashMap<(usize, usize), f32> =
            std::collections::HashMap::new();

        // 3a. Call edges.
        {
            let mut stmt = self.conn.prepare("SELECT path, name FROM call_sites")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows.flatten() {
                let Some(&caller_idx) = file_index.get(&row.0) else { continue };
                let key = row.1.to_lowercase();
                let Some(targets) = defs_by_name.get(&key) else { continue };
                if targets.is_empty() { continue }
                let share = 1.0_f32 / targets.len() as f32;
                for &t in targets {
                    if t == caller_idx { continue }  // skip self-loops
                    *edges.entry((caller_idx, t)).or_insert(0.0) += share;
                }
            }
        }

        // 3b. Import edges (weighted half — imports are weaker structural signal than calls).
        {
            let mut stmt = self.conn.prepare("SELECT path, imported_name FROM imports WHERE imported_name != ''")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows.flatten() {
                let Some(&caller_idx) = file_index.get(&row.0) else { continue };
                let key = row.1.to_lowercase();
                let Some(targets) = defs_by_name.get(&key) else { continue };
                if targets.is_empty() { continue }
                let share = 0.5_f32 / targets.len() as f32;
                for &t in targets {
                    if t == caller_idx { continue }
                    *edges.entry((caller_idx, t)).or_insert(0.0) += share;
                }
            }
        }

        // 4. Group outgoing edges by source for the PageRank loop.
        let mut out: Vec<Vec<(usize, f32)>> = vec![Vec::new(); files.len()];
        let mut out_weight: Vec<f32> = vec![0.0; files.len()];
        for ((u, v), w) in &edges {
            out[*u].push((*v, *w));
            out_weight[*u] += *w;
        }

        // 5. PageRank iterations.
        let n = files.len() as f32;
        let damping = 0.85_f32;
        let teleport = (1.0 - damping) / n;
        let mut rank = vec![1.0_f32 / n; files.len()];
        for _ in 0..25 {
            let mut new_rank = vec![teleport; files.len()];
            let mut dangling_mass = 0.0_f32;
            for (u, ow) in out_weight.iter().enumerate() {
                if *ow == 0.0 {
                    dangling_mass += rank[u];
                }
            }
            let dangling_share = damping * dangling_mass / n;
            for r in new_rank.iter_mut() { *r += dangling_share; }
            for (u, neighbors) in out.iter().enumerate() {
                let ow = out_weight[u];
                if ow == 0.0 { continue }
                let contrib = damping * rank[u] / ow;
                for &(v, w) in neighbors {
                    new_rank[v] += contrib * w;
                }
            }
            rank = new_rank;
        }

        // 6. Persist.
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM file_rank", [])?;
        for (i, p) in files.iter().enumerate() {
            tx.execute(
                "INSERT INTO file_rank (path, rank) VALUES (?1, ?2)",
                params![p, rank[i] as f64],
            )?;
        }
        tx.commit()?;
        Ok(files.len())
    }

    /// Top-N files by PageRank, descending. Returns `(path, rank)`.
    pub fn rank_files(&self, limit: usize) -> Result<Vec<(String, f32)>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, rank FROM file_rank ORDER BY rank DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)? as f32))
        })?.flatten().collect();
        Ok(rows)
    }

    /// Get rank for a single path (0.0 if unknown).
    pub fn file_rank(&self, path: &str) -> f32 {
        self.conn
            .query_row(
                "SELECT rank FROM file_rank WHERE path = ?1",
                params![path],
                |r| r.get::<_, f64>(0),
            )
            .map(|v| v as f32)
            .unwrap_or(0.0)
    }

    /// Curate a context bundle for a task within a token budget.
    /// v1 algorithm: keyword match → file scoring (matches × PageRank) → one-hop expansion via callers/importers
    /// → greedy-pack symbol signatures until budget. No file-body fetch in v1 (signatures only).
    pub fn pack_context(&self, task: &str, token_budget: usize) -> Result<PackedContext> {
        let budget_chars = token_budget.saturating_mul(4);
        let keywords = task_keywords(task);

        let mut ctx = PackedContext {
            task: task.to_string(),
            budget_chars,
            strategy: "keyword + rank + 1-hop".into(),
            ..Default::default()
        };
        if keywords.is_empty() { return Ok(ctx); }

        // 1. Score every matching symbol: token_hits × (1 + rank * 100).
        // We collect (score, Symbol) then aggregate by file.
        let mut symbol_hits: Vec<(f32, Symbol, usize)> = Vec::new();  // (score, symbol, hit_count)
        for kw in &keywords {
            let pattern = format!("%{}%", kw);
            let rows: Vec<Symbol> = self.conn
                .prepare("SELECT path, name, kind, line, signature, language, context
                           FROM symbols
                          WHERE name LIKE ?1
                          ORDER BY CASE WHEN name = ?2 COLLATE NOCASE THEN 0 ELSE 1 END
                          LIMIT 200")?
                .query_map(params![pattern, kw], row_to_symbol)?
                .flatten()
                .collect();
            for s in rows {
                let exact = s.name.eq_ignore_ascii_case(kw);
                let bump = if exact { 5.0 } else { 1.0 };
                // Merge if same symbol already scored on another keyword.
                if let Some(prev) = symbol_hits.iter_mut().find(|(_, ss, _)| ss.path == s.path && ss.name == s.name && ss.line == s.line) {
                    prev.0 += bump;
                    prev.2 += 1;
                } else {
                    symbol_hits.push((bump, s, 1));
                }
            }
        }

        // 2. Compute per-file score (sum of symbol scores × (1 + file_rank * 100)).
        let mut file_score: std::collections::HashMap<String, f32> = std::collections::HashMap::new();
        for (sc, s, _) in &symbol_hits {
            let rank = self.file_rank(&s.path);
            let weighted = *sc * (1.0 + rank * 100.0);
            *file_score.entry(s.path.clone()).or_insert(0.0) += weighted;
        }

        // 3. One-hop expansion: for each top-scoring exact-name symbol, add files containing callers + importers.
        // Keep this cheap — only the top 10 candidate symbols expand.
        let mut top_symbols_for_expand: Vec<&Symbol> = symbol_hits.iter()
            .filter(|(_, s, _)| keywords.iter().any(|k| s.name.eq_ignore_ascii_case(k)))
            .map(|(_, s, _)| s)
            .collect();
        top_symbols_for_expand.sort_by(|a, b| {
            file_score.get(&b.path).cloned().unwrap_or(0.0)
                .partial_cmp(&file_score.get(&a.path).cloned().unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top_symbols_for_expand.truncate(10);

        let mut callers_by_target: std::collections::HashMap<String, Vec<CallSite>> = std::collections::HashMap::new();
        let mut importers_by_target: std::collections::HashMap<String, Vec<Import>> = std::collections::HashMap::new();
        for s in &top_symbols_for_expand {
            let callers = self.find_references(&s.name, 5).unwrap_or_default();
            for c in &callers {
                let rank = self.file_rank(&c.path);
                *file_score.entry(c.path.clone()).or_insert(0.0) += 2.0 * (1.0 + rank * 100.0);
            }
            callers_by_target.insert(s.name.clone(), callers);

            let importers = self.importers_of(&s.name).unwrap_or_default();
            for im in &importers {
                let rank = self.file_rank(&im.path);
                *file_score.entry(im.path.clone()).or_insert(0.0) += 1.0 * (1.0 + rank * 100.0);
            }
            importers_by_target.insert(s.name.clone(), importers);
        }

        // 4. Rank files by aggregate score.
        let mut ranked_files: Vec<(String, f32)> = file_score.into_iter().collect();
        ranked_files.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // 5. Greedy pack — symbols from each top file (matched first, then near-by sigs), respecting budget.
        let mut used = 0usize;
        let mut seen_keys: std::collections::HashSet<(String, usize)> = std::collections::HashSet::new();

        for (path, _score) in &ranked_files {
            if used >= budget_chars { break }

            // Header cost is implicit in the display; we account by item size below.
            let matched_in_file: Vec<&(f32, Symbol, usize)> = symbol_hits.iter()
                .filter(|(_, s, _)| &s.path == path)
                .collect();

            for (_, s, hits) in matched_in_file {
                let key = (s.path.clone(), s.line);
                if seen_keys.contains(&key) { continue }
                let item = PackedItem {
                    path: s.path.clone(),
                    line: s.line,
                    kind: s.kind.clone(),
                    name: s.name.clone(),
                    signature: s.signature.clone(),
                    provenance: format!("name match · {} hit(s)", hits),
                };
                let cost = approx_item_cost(&item);
                if used + cost > budget_chars { break }
                used += cost;
                seen_keys.insert(key);
                ctx.items.push(item);
            }
        }

        // 6. Add caller/importer breadcrumbs from the expansion (cheap, last). 30% reserve already inside budget.
        for (target_name, callers) in &callers_by_target {
            if used >= budget_chars { break }
            for c in callers {
                let key = (c.path.clone(), c.line);
                if seen_keys.contains(&key) { continue }
                let item = PackedItem {
                    path:       c.path.clone(),
                    line:       c.line,
                    kind:       "call".into(),
                    name:       target_name.clone(),
                    signature:  if c.qualifier.is_empty() && c.receiver_expr.is_empty() {
                                    format!("{}(...)", target_name)
                                } else if !c.qualifier.is_empty() {
                                    format!("{}::{}(...)", c.qualifier, target_name)
                                } else {
                                    format!("{}.{}(...)", c.receiver_expr, target_name)
                                },
                    provenance: format!("caller of {} [{}]", target_name,
                                        if c.caller_scope.is_empty() { "<top-level>".into() } else { c.caller_scope.clone() }),
                };
                let cost = approx_item_cost(&item);
                if used + cost > budget_chars { break }
                used += cost;
                seen_keys.insert(key);
                ctx.items.push(item);
            }
        }

        for (target_name, importers) in &importers_by_target {
            if used >= budget_chars { break }
            for im in importers {
                let key = (im.path.clone(), im.line);
                if seen_keys.contains(&key) { continue }
                let item = PackedItem {
                    path:       im.path.clone(),
                    line:       im.line,
                    kind:       "import".into(),
                    name:       im.imported_name.clone(),
                    signature:  if im.alias.is_empty() {
                                    format!("use {}::{}", im.module, im.imported_name)
                                } else {
                                    format!("use {}::{} as {}", im.module, im.imported_name, im.alias)
                                },
                    provenance: format!("importer of {}", target_name),
                };
                let cost = approx_item_cost(&item);
                if used + cost > budget_chars { break }
                used += cost;
                seen_keys.insert(key);
                ctx.items.push(item);
            }
        }

        // Stable display order: by path, then line.
        ctx.items.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.line.cmp(&b.line)));
        ctx.total_chars = used;
        Ok(ctx)
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
