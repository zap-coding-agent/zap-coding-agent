use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::tools::Tool;
use super::{guard_path, print_diff};

// ── shared whitespace-stripping fallback ──────────────────────────────────────

/// When an exact string match against `content` fails, try stripping leading
/// whitespace from each line of `old_string`.  Only succeeds if the stripped
/// form gives exactly 1 match (ambiguous files are never silently changed).
///
/// Recovers from the common LLM mistake of carrying indentation from
/// read_file's line-number prefix (`"  1 | "`) into old_string.
fn whitespace_fallback(content: &str, old_string: &str) -> Option<String> {
    let stripped = old_string
        .lines()
        .map(|l| l.trim_start())
        .collect::<Vec<_>>()
        .join("\n");
    if stripped == old_string {
        return None; // no leading whitespace to strip — don't bother
    }
    if content.matches(stripped.as_str()).count() == 1 {
        Some(stripped)
    } else {
        None
    }
}

// ── edit_file ─────────────────────────────────────────────────────────────────

pub struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str { "edit_file" }
    fn description(&self) -> &str {
        "Make a surgical edit to a file by replacing an exact string with a new one. \
         The old_string must match exactly (including whitespace and indentation). \
         If old_string appears more than once and replace_all is false, the call is \
         rejected — add more surrounding context to make it unique. \
         Always include expected_line (1-based, as shown in read_file output) so the \
         tool verifies the match is at the correct location before mutating anything."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":        { "type": "string",  "description": "File to edit." },
                "old_string":  { "type": "string",  "description": "Exact text to find and replace." },
                "new_string":  { "type": "string",  "description": "Text to replace it with." },
                "replace_all": { "type": "boolean", "description": "Replace every occurrence (default false)." },
                "expected_line": { "type": "integer", "description": "Optional: 1-based line number where old_string should start. Tool verifies match position before editing." }
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
        let expected_line = input["expected_line"].as_u64().map(|n| n as usize);

        let _ = crate::snapshot::save_snapshot(path);

        let content = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("edit_file: cannot read '{}'", path))?;

        let count = content.matches(old_string).count();
        let (effective_old, count) = if count == 0 {
            if let Some(fallback) = whitespace_fallback(&content, old_string) {
                (fallback, 1usize)
            } else {
                (old_string.to_string(), 0usize)
            }
        } else {
            (old_string.to_string(), count)
        };
        let old_string = effective_old.as_str();

        if count == 0 {
            let preview = make_preview(old_string);
            anyhow::bail!(
                "edit_file: old_string not found in '{}'. \
                 Searched for: {}. \
                 Make sure the text matches exactly (including whitespace and indentation). \
                 Hint: use shell + cat -A or python3 repr() to check the file's real bytes.",
                path, preview
            );
        }
        if count > 1 && !replace_all {
            anyhow::bail!(
                "edit_file: old_string appears {} times in '{}'. \
                 Add more surrounding context to make it unique, or set replace_all=true.",
                count, path
            );
        }

        // Positional guard: verify match starts on expected line (1-based)
        if let Some(expected) = expected_line {
            let pos = content.find(old_string).unwrap(); // safe: count > 0
            let actual_line = content[..pos].chars().filter(|&c| c == '\n').count() + 1;
            if actual_line != expected {
                anyhow::bail!(
                    "edit_file: old_string matched at line {} but you expected line {}. \
                     Your old_string is correct content but it appears at a different location \
                     than you intended. Re-read the file with read_file and use the correct \
                     line number.",
                    actual_line, expected
                );
            }
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

// ── batch_edit ────────────────────────────────────────────────────────────────

pub struct BatchEditTool;

#[async_trait]
impl Tool for BatchEditTool {
    fn name(&self) -> &str { "batch_edit" }
    fn description(&self) -> &str {
        "Apply multiple surgical edits to a single file in one call. \
         Each edit replaces an exact old_string with a new_string, applied in order. \
         All edits are validated before any are applied. \
         A single colored diff is shown at the end. \
         Include expected_line per edit (1-based, from read_file output) for positional safety."
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
                            "replace_all": { "type": "boolean", "description": "Replace every occurrence (default false)." },
                            "expected_line": { "type": "integer", "description": "Optional: 1-based line where old_string should start." }
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

        // Validate all edits first, collecting effective old_strings (with whitespace fallback).
        let mut effective_olds: Vec<String> = Vec::with_capacity(edits.len());
        for (i, edit) in edits.iter().enumerate() {
            let old = edit["old_string"].as_str()
                .with_context(|| format!("batch_edit: edit[{}] missing old_string", i))?;
            let replace_all = edit["replace_all"].as_bool().unwrap_or(false);
            let count = original.matches(old).count();
            let (effective_old, count) = if count == 0 {
                if let Some(fallback) = whitespace_fallback(&original, old) {
                    (fallback, 1usize)
                } else {
                    (old.to_string(), 0usize)
                }
            } else {
                (old.to_string(), count)
            };
            let old = effective_old.as_str();

            if count == 0 {
                anyhow::bail!(
                    "batch_edit: edit[{}] old_string not found in '{}'. Searched for: {}",
                    i, path, make_preview(old)
                );
            }
            if count > 1 && !replace_all {
                anyhow::bail!(
                    "batch_edit: edit[{}] old_string appears {} times in '{}'. \
                     Add more context or set replace_all=true.",
                    i, count, path
                );
            }

            // Positional guard per edit
            if let Some(expected) = edit["expected_line"].as_u64().map(|n| n as usize) {
                let pos = original.find(old).unwrap(); // safe: count > 0
                let actual_line = original[..pos].chars().filter(|&c| c == '\n').count() + 1;
                if actual_line != expected {
                    anyhow::bail!(
                        "batch_edit: edit[{}] matched at line {} but expected line {}. \
                         Re-read the file and use the correct line number from read_file output.",
                        i, actual_line, expected
                    );
                }
            }

            effective_olds.push(effective_old);
        }

        let mut content = original.clone();
        let mut total_replacements = 0usize;
        for (edit, effective_old) in edits.iter().zip(effective_olds.iter()) {
            let old = effective_old.as_str();
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

// ── shared preview helper ─────────────────────────────────────────────────────

fn make_preview(s: &str) -> String {
    let h: String = s.chars().take(40).map(|c|
        match c { '\n' => '↵', '\r' => '←', '\t' => '→', c => c }
    ).collect();
    let t: String = if s.chars().count() > 50 {
        s.chars().rev().take(30).collect::<String>().chars().rev().map(|c|
            match c { '\n' => '↵', '\r' => '←', '\t' => '→', c => c }
        ).collect()
    } else {
        String::new()
    };
    if t.is_empty() { format!("`{}`", h) } else { format!("`{}…{}`", h, t) }
}
