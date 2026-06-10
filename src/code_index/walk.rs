use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::Symbol;

pub(super) fn detect_language(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs")              => "rust",
        Some("py")              => "python",
        Some("js") | Some("jsx") => "javascript",
        Some("ts")              => "typescript",
        Some("tsx")             => "tsx",
        Some("go")              => "go",
        Some("java")            => "java",
        Some("cs")              => "csharp",
        _                       => "",
    }
}

pub(super) fn walkdir_filtered(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with('.') || matches!(name,
                    "node_modules" | "dist" | "build" | "coverage" | ".next" | ".nuxt" |
                    "target" |
                    "vendor" | "__pycache__" | ".venv" | "venv" | "site-packages" |
                    "bin" | "obj" |
                    "out" | ".gradle" | ".mvn" |
                    "tmp" | "temp" | "logs"
                ) {
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

pub(super) fn file_mtime(path: &Path) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

pub(super) fn row_to_symbol(row: &rusqlite::Row) -> rusqlite::Result<Symbol> {
    Ok(Symbol {
        path:        row.get(0)?,
        name:        row.get(1)?,
        kind:        row.get(2)?,
        line:        row.get::<_, i64>(3)? as usize,
        signature:   row.get(4)?,
        language:    row.get(5)?,
        context:     row.get(6)?,
        return_type: row.get(7).unwrap_or_default(),
        params:      row.get(8).unwrap_or_default(),
    })
}

pub(super) fn row_to_call_site(row: &rusqlite::Row) -> rusqlite::Result<super::CallSite> {
    Ok(super::CallSite {
        path:          row.get(0)?,
        line:          row.get::<_, i64>(1)? as usize,
        col:           row.get::<_, i64>(2)? as usize,
        name:          row.get(3)?,
        qualifier:     row.get(4)?,
        receiver_expr: row.get(5)?,
        caller_scope:  row.get(6)?,
        language:      row.get(7)?,
    })
}

pub(super) fn row_to_import(row: &rusqlite::Row) -> rusqlite::Result<super::Import> {
    Ok(super::Import {
        path:          row.get(0)?,
        line:          row.get::<_, i64>(1)? as usize,
        module:        row.get(2)?,
        imported_name: row.get(3)?,
        alias:         row.get(4)?,
        language:      row.get(5)?,
    })
}
