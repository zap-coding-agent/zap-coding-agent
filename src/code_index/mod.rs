use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

mod extract;
mod extract_csharp;
mod extract_go;
mod extract_java;
mod extract_js;
mod extract_python;
mod extract_rust;
mod index_impl;
mod index_pack;
mod index_quality;
mod index_query;
mod index_rank;
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

#[derive(Debug, Clone)]
pub struct CallSite {
    pub path:          String,
    pub line:          usize,
    pub col:           usize,
    pub name:          String,
    pub qualifier:     String,
    pub receiver_expr: String,
    pub caller_scope:  String,
    pub language:      String,
}

impl CallSite {
    pub fn display(&self) -> String {
        let scope = if self.caller_scope.is_empty() { "<top-level>".to_string() } else { self.caller_scope.clone() };
        let full_name = if !self.qualifier.is_empty() {
            format!("{}::{}", self.qualifier, self.name)
        } else if !self.receiver_expr.is_empty() {
            format!("{}.{}", self.receiver_expr, self.name)
        } else {
            self.name.clone()
        };
        format!("{}:{} [{}] → {}", self.path, self.line, scope, full_name)
    }
}

#[derive(Debug, Clone)]
pub struct Import {
    pub path:          String,
    pub line:          usize,
    pub module:        String,
    pub imported_name: String,
    pub alias:         String,
    pub language:      String,
}

impl Import {
    pub fn display(&self) -> String {
        let what = if self.imported_name.is_empty() {
            self.module.clone()
        } else if self.module.is_empty() {
            self.imported_name.clone()
        } else {
            format!("{}::{}", self.module, self.imported_name)
        };
        if self.alias.is_empty() {
            format!("{}:{} {}", self.path, self.line, what)
        } else {
            format!("{}:{} {} as {}", self.path, self.line, what, self.alias)
        }
    }
}

/// Whether graph data (call_sites + imports) is emitted on index.
/// `ZAP_INDEX_MODE=symbols` falls back to symbol-only mode (B-tier).
/// Default = graph (A-tier).
pub fn graph_enabled() -> bool {
    !matches!(std::env::var("ZAP_INDEX_MODE").ok().as_deref(), Some("symbols") | Some("ast"))
}

/// One element of a packed-context bundle returned by `pack_context`.
#[derive(Debug, Clone)]
pub struct PackedItem {
    pub path:       String,
    pub line:       usize,
    pub kind:       String,       // "fn", "struct", "class" … (same vocabulary as Symbol.kind)
    pub name:       String,
    pub signature:  String,
    pub provenance: String,       // why this row is in the bundle
}

#[derive(Debug, Clone, Default)]
pub struct PackedContext {
    pub items:        Vec<PackedItem>,
    pub total_chars:  usize,
    pub strategy:     String,
    pub budget_chars: usize,
    pub task:         String,
}

impl PackedContext {
    /// Crude tokens estimate (chars / 4).
    pub fn total_tokens_est(&self) -> usize { self.total_chars / 4 }

    pub fn to_display(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "# Packed context for: \"{}\"\nstrategy: {} · budget: ~{} tokens · used: ~{} tokens · {} item(s)\n\n",
            self.task,
            self.strategy,
            self.budget_chars / 4,
            self.total_tokens_est(),
            self.items.len(),
        ));
        if self.items.is_empty() {
            out.push_str("(No matches found in the code index for this task.)\n");
            return out;
        }
        let mut current_path = String::new();
        for it in &self.items {
            if it.path != current_path {
                if !current_path.is_empty() { out.push('\n'); }
                out.push_str(&format!("## {}\n", it.path));
                current_path = it.path.clone();
            }
            out.push_str(&format!(
                "  L{:>5} [{}] {} — {}  ({})\n",
                it.line, it.kind, it.name, it.signature, it.provenance
            ));
        }
        out
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

pub fn global_find_references(name: &str, limit: usize) -> Vec<CallSite> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.find_references(name, limit).ok())
        .unwrap_or_default()
}

pub fn global_callers_of(name: &str, qualifier: Option<&str>, limit: usize) -> Vec<CallSite> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.callers_of(name, qualifier, limit).ok())
        .unwrap_or_default()
}

pub fn global_imports_for(path: &str) -> Vec<Import> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.imports_for(path).ok())
        .unwrap_or_default()
}

pub fn global_importers_of(name: &str) -> Vec<Import> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.importers_of(name).ok())
        .unwrap_or_default()
}

pub fn global_users_of_module(module: &str) -> Vec<Import> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.users_of_module(module).ok())
        .unwrap_or_default()
}

pub fn global_rank_files(limit: usize) -> Vec<(String, f32)> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.rank_files(limit).ok())
        .unwrap_or_default()
}

pub fn global_file_rank(path: &str) -> f32 {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .map(|g| g.file_rank(path))
        .unwrap_or(0.0)
}

pub fn global_pack_context(task: &str, token_budget: usize) -> Option<PackedContext> {
    GLOBAL_INDEX
        .get()
        .and_then(|g| g.lock().ok())
        .and_then(|g| g.pack_context(task, token_budget).ok())
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
        let mut consecutive_errors = 0u32;
        loop {
            interval.tick().await;
            let Some(g) = GLOBAL_INDEX.get() else { continue };
            let Ok(mut guard) = g.try_lock() else { continue };
            // index_dir is synchronous (tree-sitter parsing + SQLite writes).
            // block_in_place tells tokio to spin up a replacement worker thread
            // so the async pool stays at full capacity while this runs.
            let result = tokio::task::block_in_place(|| guard.index_dir(&cwd));
            match result {
                Ok((files, symbols)) if files > 0 => {
                    consecutive_errors = 0;
                    crate::log::write(
                        "INDEX",
                        &format!("tree-sitter · background · {} files updated · {} symbols", files, symbols),
                    );
                }
                Ok(_) => { consecutive_errors = 0; }
                Err(e) => {
                    consecutive_errors += 1;
                    crate::log::write("WARN ", &format!("background index error: {}", e));
                    if consecutive_errors >= 3 {
                        crate::log::write("WARN ", "background indexer: stopping after 3 consecutive failures (check .zap/code.db)");
                        break;
                    }
                }
            }
        }
    });
}

/// Build the index for the current directory and print stats.
/// Used by `zap --index-only` — no session, no LLM call.
pub fn run_index_standalone() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    println!("  → indexing {} …", cwd.display());
    let mut index = CodeIndex::open(&cwd)?;
    let (files, syms) = index.index_dir(&cwd)?;
    println!("  ✓ {} file(s) · {} symbol(s) indexed  (.zap/code.db)", files, syms);
    Ok(())
}

// ── CodeIndex ─────────────────────────────────────────────────────────────────

pub struct CodeIndex {
    conn: rusqlite::Connection,
    db_path: PathBuf,
}
