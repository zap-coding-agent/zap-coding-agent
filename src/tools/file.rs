use anyhow::{Context, Result};
use async_trait::async_trait;
use colored::Colorize;
use similar::{ChangeTag, TextDiff};

use super::Tool;

// ── Path safety guard ─────────────────────────────────────────────────────────

/// Normalize a path string to an absolute path without requiring it to exist
/// (unlike std::fs::canonicalize). Resolves `.` and `..` components.
fn normalize_path(path: &str) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};
    let base = if std::path::Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut out = PathBuf::new();
    for c in base.components() {
        match c {
            Component::ParentDir => { out.pop(); }
            Component::CurDir    => {}
            other                => out.push(other),
        }
    }
    out
}

/// Reject paths that point at known-sensitive locations (credentials, keys, config).
/// Called before every file read or write.
fn guard_path(path: &str) -> Result<()> {
    let abs = normalize_path(path);
    let abs_str = abs.to_string_lossy().to_lowercase();

    const BLOCKED_SEGMENTS: &[&str] = &[
        "/.ssh/", "/.aws/", "/.gnupg/", "/.kube/", "/.docker/",
        "/.config/gcloud", "/.netrc", "/.git-credentials", "/.pgpass",
        "/etc/passwd", "/etc/shadow", "/etc/sudoers",
    ];
    for seg in BLOCKED_SEGMENTS {
        if abs_str.contains(seg) {
            anyhow::bail!(
                "security: access to '{}' is blocked (sensitive path). \
                 Use the shell tool if access is intentional.",
                path
            );
        }
    }

    // Block ~/.agent.toml — contains API keys.
    if let Some(home) = dirs::home_dir() {
        if abs == home.join(".agent.toml") {
            anyhow::bail!(
                "security: '{}' is blocked — it contains API keys.",
                path
            );
        }
    }

    Ok(())
}

// ── read_file ─────────────────────────────────────────────────────────────────

pub(super) struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str {
        "Read a file's contents, with optional line range. \
         Output is prefixed with 1-based line numbers (same as cat -n) so you \
         can reference exact lines in subsequent edit_file calls. \
         For large files, use offset + limit to read only the relevant section."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":   { "type": "string", "description": "Path to the file to read." },
                "offset": { "type": "integer", "description": "First line to read (0-based, default 0)." },
                "limit":  { "type": "integer", "description": "Maximum number of lines to return." }
            },
            "required": ["path"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("read '{}'", input["path"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"]
            .as_str()
            .context("read_file: 'path' must be a string")?;
        guard_path(path)?;

        let raw = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("read_file: cannot read '{}'", path))?;

        let offset = input["offset"].as_u64().unwrap_or(0) as usize;
        let limit  = input["limit"].as_u64().map(|l| l as usize);

        let lines: Vec<&str> = raw.lines().collect();
        let total = lines.len();
        let start = offset.min(total);
        let end   = limit.map(|l| (start + l).min(total)).unwrap_or(total);

        if start == end {
            return Ok(format!("(file '{}' has {} lines; offset {} is past the end)", path, total, offset));
        }

        let out = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{}\t{}", start + i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(out)
    }
}

// ── edit_file ─────────────────────────────────────────────────────────────────

pub(super) struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str { "edit_file" }
    fn description(&self) -> &str {
        "Make a surgical edit to a file by replacing an exact string with a new one. \
         The old_string must match exactly (including whitespace and indentation). \
         If old_string appears more than once and replace_all is false, the call is \
         rejected — add more surrounding context to make it unique."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":        { "type": "string",  "description": "File to edit." },
                "old_string":  { "type": "string",  "description": "Exact text to find and replace." },
                "new_string":  { "type": "string",  "description": "Text to replace it with." },
                "replace_all": { "type": "boolean", "description": "Replace every occurrence (default false)." }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("edit '{}': replace old_string with new_string",
            input["path"].as_str().unwrap_or("?"))
    }
    fn affected_path<'a>(&self, input: &'a serde_json::Value) -> Option<&'a str> {
        input["path"].as_str()
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"]
            .as_str()
            .context("edit_file: 'path' must be a string")?;
        guard_path(path)?;
        let old_string = input["old_string"]
            .as_str()
            .context("edit_file: 'old_string' must be a string")?;
        let new_string = input["new_string"]
            .as_str()
            .context("edit_file: 'new_string' must be a string")?;
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

        let _ = crate::snapshot::save_snapshot(path);

        let content = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("edit_file: cannot read '{}'", path))?;

        let count = content.matches(old_string).count();
        if count == 0 {
            anyhow::bail!(
                "edit_file: old_string not found in '{}'. \
                 Make sure the text matches exactly (including whitespace and indentation).",
                path
            );
        }
        if count > 1 && !replace_all {
            anyhow::bail!(
                "edit_file: old_string appears {} times in '{}'. \
                 Add more surrounding context to make it unique, or set replace_all=true.",
                count, path
            );
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        tokio::fs::write(path, &new_content)
            .await
            .with_context(|| format!("edit_file: cannot write '{}'", path))?;

        if !crate::tui::channel::is_tui_mode() {
            print_diff(content.as_str(), new_content.as_str());
        }

        Ok(format!(
            "edited '{}': replaced {} occurrence(s) ({} → {} bytes)",
            path, count, content.len(), new_content.len()
        ))
    }
}

// ── write_file ────────────────────────────────────────────────────────────────

pub(super) struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str { "write_file" }
    fn description(&self) -> &str {
        "Write content to a file, creating it or overwriting it. \
         Requires user approval before executing."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":    { "type": "string", "description": "Destination file path." },
                "content": { "type": "string", "description": "Content to write." }
            },
            "required": ["path", "content"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        let path = input["path"].as_str().unwrap_or("?");
        let bytes = input["content"].as_str().map(|s| s.len()).unwrap_or(0);
        format!("write {} bytes to '{}'", bytes, path)
    }
    fn affected_path<'a>(&self, input: &'a serde_json::Value) -> Option<&'a str> {
        input["path"].as_str()
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"]
            .as_str()
            .context("write_file: 'path' must be a string")?;
        guard_path(path)?;
        let content = input["content"]
            .as_str()
            .context("write_file: 'content' must be a string")?;

        let _ = crate::snapshot::save_snapshot(path);

        let abs_path = normalize_path(path);
        if let Some(parent) = abs_path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!(
                        "write_file: cannot create dirs '{}' (resolved to '{}')",
                        path, parent.display()
                    ))?;
            }
        }

        tokio::fs::write(&abs_path, content)
            .await
            .with_context(|| format!(
                "write_file: cannot write '{}' (resolved to '{}')",
                path, abs_path.display()
            ))?;

        Ok(format!("wrote {} bytes to '{}'", content.len(), path))
    }
}

// ── batch_edit ────────────────────────────────────────────────────────────────

pub(super) struct BatchEditTool;

#[async_trait]
impl Tool for BatchEditTool {
    fn name(&self) -> &str { "batch_edit" }
    fn description(&self) -> &str {
        "Apply multiple surgical edits to a single file in one call. \
         Each edit replaces an exact old_string with a new_string, applied in order. \
         All edits are validated before any are applied. \
         A single colored diff is shown at the end."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File to edit." },
                "edits": {
                    "type": "array",
                    "description": "Ordered list of edits to apply.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "old_string": { "type": "string", "description": "Exact text to find." },
                            "new_string": { "type": "string", "description": "Replacement text." },
                            "replace_all": { "type": "boolean", "description": "Replace every occurrence (default false)." }
                        },
                        "required": ["old_string", "new_string"]
                    }
                }
            },
            "required": ["path", "edits"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        let path = input["path"].as_str().unwrap_or("?");
        let n = input["edits"].as_array().map(|a| a.len()).unwrap_or(0);
        format!("batch edit '{}': {} edit(s)", path, n)
    }
    fn affected_path<'a>(&self, input: &'a serde_json::Value) -> Option<&'a str> {
        input["path"].as_str()
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"].as_str().context("batch_edit: 'path' required")?;
        guard_path(path)?;
        let edits = input["edits"].as_array().context("batch_edit: 'edits' required")?;

        let _ = crate::snapshot::save_snapshot(path);
        let original = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("batch_edit: cannot read '{}'", path))?;

        for (i, edit) in edits.iter().enumerate() {
            let old = edit["old_string"].as_str()
                .with_context(|| format!("batch_edit: edit[{}] missing old_string", i))?;
            let replace_all = edit["replace_all"].as_bool().unwrap_or(false);
            let count = original.matches(old).count();
            if count == 0 {
                anyhow::bail!("batch_edit: edit[{}] old_string not found in '{}'", i, path);
            }
            if count > 1 && !replace_all {
                anyhow::bail!(
                    "batch_edit: edit[{}] old_string appears {} times in '{}'. \
                     Add more context or set replace_all=true.",
                    i, count, path
                );
            }
        }

        let mut content = original.clone();
        let mut total_replacements = 0usize;
        for edit in edits {
            let old = edit["old_string"].as_str().unwrap();
            let new = edit["new_string"].as_str().unwrap_or("");
            let replace_all = edit["replace_all"].as_bool().unwrap_or(false);
            let count = content.matches(old).count();
            content = if replace_all {
                content.replace(old, new)
            } else {
                content.replacen(old, new, 1)
            };
            total_replacements += count;
        }

        tokio::fs::write(path, &content)
            .await
            .with_context(|| format!("batch_edit: cannot write '{}'", path))?;

        if !crate::tui::channel::is_tui_mode() {
            print_diff(original.as_str(), content.as_str());
        }

        Ok(format!(
            "batch_edit '{}': {} edit(s) applied, {} replacement(s) ({} → {} bytes)",
            path, edits.len(), total_replacements, original.len(), content.len()
        ))
    }
}

// ── undo_edit ─────────────────────────────────────────────────────────────────

pub(super) struct UndoEditTool;

#[async_trait]
impl Tool for UndoEditTool {
    fn name(&self) -> &str { "undo_edit" }
    fn description(&self) -> &str {
        "Revert the most recent edit or write to a file, restoring its previous content. \
         Multiple undos are supported (one per previous edit). \
         Use 'list' as the path to see which files have snapshots available."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File to undo edits on, or 'list' to see available snapshots." }
            },
            "required": ["path"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("undo edit on '{}'", input["path"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"].as_str().context("undo_edit: 'path' required")?;
        if path == "list" {
            let snaps = crate::snapshot::list_snapshots();
            if snaps.is_empty() {
                return Ok("No snapshots available (no edits made this session).".to_string());
            }
            return Ok(format!("Files with undo snapshots:\n{}", snaps.join("\n")));
        }
        let restored = crate::snapshot::restore_snapshot(path)?;
        Ok(format!(
            "Restored '{}' to previous version ({} bytes).",
            path, restored.len()
        ))
    }
}

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

pub(super) struct GlobReadTool;

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
        let opts = glob::MatchOptions::new(); // default: dots matched, case-sensitive
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

// ── shared diff printer ────────────────────────────────────────────────────────

fn print_diff(before: &str, after: &str) {
    let diff = TextDiff::from_lines(before, after);
    let mut had_diff = false;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Delete => {
                if !had_diff {
                    println!("  {}", "─── diff ──────────────────────────────────".dimmed());
                    had_diff = true;
                }
                print!("{}", format!("  - {}", change.value()).red());
            }
            ChangeTag::Insert => {
                if !had_diff {
                    println!("  {}", "─── diff ──────────────────────────────────".dimmed());
                    had_diff = true;
                }
                print!("{}", format!("  + {}", change.value()).green());
            }
            ChangeTag::Equal => {}
        }
    }
    if had_diff {
        println!("  {}", "───────────────────────────────────────────".dimmed());
    }
}
