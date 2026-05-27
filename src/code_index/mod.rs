use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

mod extract;
mod index_impl;
mod walk;

// ── Public types ──────────────────────────────────────────────────────────────

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

pub fn global_index() -> Option<Arc<Mutex<CodeIndex>>> {
    GLOBAL_INDEX.get().cloned()
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
pub fn global_file_line_counts() -> Vec<(String, usize, usize)> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
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
pub fn spawn_background_indexer(cwd: PathBuf) {
    tokio::spawn(async move {
        let secs = std::env::var("ZAP_INDEX_INTERVAL")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(120);
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(secs));
        interval.tick().await;
        loop {
            interval.tick().await;
            let Some(g) = GLOBAL_INDEX.get() else { continue };
            let Ok(mut guard) = g.try_lock() else { continue };
            match guard.index_dir(&cwd) {
                Ok((files, symbols)) if files > 0 => {
                    crate::log::write(
                        "INDEX",
                        &format!("tree-sitter · background · {} files updated · {} symbols", files, symbols),
                    );
                }
                Ok(_) => {}
                Err(e) => crate::log::write("WARN ", &format!("background index error: {}", e)),
            }
        }
    });
}

// ── CodeIndex ─────────────────────────────────────────────────────────────────

pub struct CodeIndex {
    conn: rusqlite::Connection,
    db_path: PathBuf,
}
