use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::tools::Tool;

// ── glob_walk_safe ─────────────────────────────────────────────────────────────

/// Symlink-safe recursive directory walker for glob_read.
///
/// Unlike glob::glob(), this never follows symlinks, so directory cycles like
/// `.kiro/skills/.kiro → .kiro` terminate cleanly. Real hidden directories
/// (`.kiro`, `.claude`, etc.) are walked normally — only symlinks are skipped.
fn glob_walk_safe(
    dir:     &std::path::Path,
    base:    &std::path::Path,
    pattern: &glob::Pattern,
    opts:    glob::MatchOptions,
    results: &mut Vec<std::path::PathBuf>,
    max:     usize,
    depth:   usize,
) {
    const MAX_DEPTH: usize = 30;
    if results.len() >= max || depth > MAX_DEPTH { return; }

    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        if results.len() >= max { return; }
        let path = entry.path();

        // Skip all symlinks — they are the only source of directory cycles.
        // A real .kiro/ or .claude/ directory is NOT a symlink and will be walked.
        if path.is_symlink() { continue; }

        if path.is_dir() {
            // Skip build / vendor noise that is never useful to glob.
            let name = entry.file_name();
            let n = name.to_string_lossy();
            if matches!(n.as_ref(),
                "target" | "node_modules" | "vendor" | "dist" | "build"
                | "bin" | "obj" | "out" | "__pycache__" | ".git" | ".svn" | ".hg"
                | ".venv" | "venv" | "site-packages" | "coverage" | ".next" | ".nuxt"
            ) { continue; }
            glob_walk_safe(&path, base, pattern, opts, results, max, depth + 1);
        } else if path.is_file() {
            if let Ok(rel) = path.strip_prefix(base) {
                if pattern.matches_path_with(rel, opts) {
                    results.push(path);
                }
            }
        }
    }
}

// ── glob_read ─────────────────────────────────────────────────────────────────

pub struct GlobReadTool;

#[async_trait]
impl Tool for GlobReadTool {
    fn name(&self) -> &str { "glob_read" }
    fn description(&self) -> &str {
        "List and optionally read files matching a glob pattern (e.g. 'src/**/*.rs'). \
         Returns file paths and the first `preview_lines` lines of each file. \
         Use this to survey multiple files quickly without reading each one individually."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern":       { "type": "string",  "description": "Glob pattern, e.g. 'src/**/*.rs' or 'tests/*.py'." },
                "preview_lines": { "type": "integer", "description": "Lines to preview per file (default 0 = names only)." },
                "max_files":     { "type": "integer", "description": "Maximum number of files to return (default 30)." }
            },
            "required": ["pattern"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("glob '{}'", input["pattern"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let pattern   = input["pattern"].as_str().context("glob_read: 'pattern' required")?;
        let preview   = input["preview_lines"].as_u64().unwrap_or(0) as usize;
        let max_files = input["max_files"].as_u64().unwrap_or(30) as usize;

        // Use a symlink-safe custom walker instead of glob::glob().
        // glob::glob() has no cycle detection: if a symlink creates a loop
        // (e.g. .kiro/skills/.kiro → .kiro) the iterator spins forever and
        // take(max_files) never fires because results are never yielded.
        // Real hidden directories (.kiro, .claude, etc.) are still walked normally.
        let pat = glob::Pattern::new(pattern)
            .with_context(|| format!("glob_read: invalid pattern '{}'", pattern))?;
        let opts = glob::MatchOptions::new();
        let cwd = std::env::current_dir()?;
        let mut paths: Vec<std::path::PathBuf> = Vec::new();
        glob_walk_safe(&cwd, &cwd, &pat, opts, &mut paths, max_files, 0);
        paths.sort();

        if paths.is_empty() {
            return Ok(format!("No files match '{}'", pattern));
        }

        let mut out = format!("{} file(s) matching '{}':\n", paths.len(), pattern);
        for path in &paths {
            let display = path.display().to_string();
            if preview == 0 {
                out.push_str(&format!("  {}\n", display));
            } else {
                out.push_str(&format!("\n-- {} --\n", display));
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        for (i, line) in content.lines().take(preview).enumerate() {
                            out.push_str(&format!("{}\t{}\n", i + 1, line));
                        }
                        let total = content.lines().count();
                        if total > preview {
                            out.push_str(&format!("  ... ({} more lines)\n", total - preview));
                        }
                    }
                    Err(e) => out.push_str(&format!("  [error: {}]\n", e)),
                }
            }
        }
        Ok(out)
    }
}
