use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::Symbol;

pub(super) fn count_call_sites(source: &str, freq: &mut std::collections::HashMap<String, usize>) {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut i = 0usize;
    while i < len {
        let b = bytes[i];
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'/' {
            while i < len && bytes[i] != b'\n' { i += 1; }
            continue;
        }
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') { i += 1; }
            i += 2;
            continue;
        }
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
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') { i += 1; }
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

pub(super) fn detect_language(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs")              => "rust",
        Some("py")              => "python",
        Some("js") | Some("jsx") => "javascript",
        Some("ts")              => "typescript",
        Some("tsx")             => "tsx",
        Some("go")              => "go",
        Some("java")            => "java",
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
        path:      row.get(0)?,
        name:      row.get(1)?,
        kind:      row.get(2)?,
        line:      row.get::<_, i64>(3)? as usize,
        signature: row.get(4)?,
        language:  row.get(5)?,
        context:   row.get(6)?,
    })
}
