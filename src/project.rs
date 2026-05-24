//! Project-level state persisted in `.zap/` inside the current working directory.
//!
//! Unlike `~/.zap/agent.db` (global, all projects), these files are project-specific:
//!   .zap/project.json     — language, index status, init state
//!   .zap/context.md       — last-session handoff (goal, files touched)
//!   .zap/session_log.md   — one entry per session: intent + files
//!   .zap/understanding.md — LLM-maintained project knowledge (written by /init)

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Returns `.zap/` path in CWD, creating it if necessary.
pub fn zap_dir() -> PathBuf {
    let dir = PathBuf::from(".zap");
    std::fs::create_dir_all(&dir).ok();
    dir
}

// ── project.json ──────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ProjectMeta {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub language: Vec<String>,
    #[serde(default)]
    pub indexed: bool,
    #[serde(default)]
    pub indexed_at: Option<String>,
    #[serde(default)]
    pub initialized_at: Option<String>,
}

pub fn load_project_meta() -> Option<ProjectMeta> {
    let path = PathBuf::from(".zap").join("project.json");
    let contents = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&contents).ok()
}

pub fn save_project_meta(meta: &ProjectMeta) -> Result<()> {
    let path = zap_dir().join("project.json");
    std::fs::write(&path, serde_json::to_string_pretty(meta)?)?;
    Ok(())
}

/// Mark the project as indexed (called after a successful /index run).
pub fn mark_indexed() {
    if let Some(mut meta) = load_project_meta() {
        meta.indexed = true;
        meta.indexed_at = Some(Utc::now().to_rfc3339());
        let _ = save_project_meta(&meta);
    }
}

// ── context.md ────────────────────────────────────────────────────────────────

/// Returns the raw content of `.zap/context.md` if it exists and is non-empty.
pub fn load_session_context() -> Option<String> {
    let s = std::fs::read_to_string(PathBuf::from(".zap").join("context.md")).ok()?;
    if s.trim().is_empty() { None } else { Some(s) }
}

/// Extract just the "What was being worked on" line for the startup banner.
pub fn context_summary() -> Option<String> {
    let raw = load_session_context()?;
    // Find the line after "## What was being worked on"
    let mut found = false;
    for line in raw.lines() {
        if found {
            let trimmed = line.trim();
            if !trimmed.is_empty() && !trimmed.starts_with("<!--") {
                return Some(trimmed.to_string());
            }
        }
        if line.starts_with("## What was being worked on") {
            found = true;
        }
    }
    None
}

/// Extract the files list from context.md for the startup banner.
pub fn context_files() -> Vec<String> {
    let raw = match load_session_context() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let mut in_files = false;
    let mut files = Vec::new();
    for line in raw.lines() {
        if line.starts_with("## Files touched") {
            in_files = true;
            continue;
        }
        if in_files {
            if line.starts_with("## ") { break; }
            let trimmed = line.trim().trim_start_matches("- ");
            if !trimmed.is_empty() && trimmed != "(none)" && !trimmed.starts_with("<!--") {
                files.push(trimmed.to_string());
            }
        }
    }
    files
}

/// Write `.zap/context.md` at session end.
pub fn save_session_context(session_id: i64, goal: &str, files_changed: &[String]) -> Result<()> {
    let now = Utc::now().format("%Y-%m-%d %H:%M").to_string();
    let files_section = if files_changed.is_empty() {
        "  (none)".to_string()
    } else {
        // Deduplicate preserving order
        let mut seen = std::collections::HashSet::new();
        files_changed.iter()
            .filter(|f| seen.insert(*f))
            .map(|f| format!("  - {}", f))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let content = format!(
        "# Session Context\n\
         \n\
         <!-- auto-written by zap at session end — edit freely -->\n\
         \n\
         ## Last updated\n\
         {now} — Session #{session_id}\n\
         \n\
         ## What was being worked on\n\
         {goal}\n\
         \n\
         ## Files touched\n\
         {files_section}\n\
         \n\
         ## What's next\n\
         <!-- fill this in between sessions -->\n"
    );
    std::fs::write(zap_dir().join("context.md"), content)?;
    Ok(())
}

// ── session_log.md ────────────────────────────────────────────────────────────

/// Prepend one entry to `.zap/session_log.md` (newest first, capped at ~20k chars).
pub fn append_session_log(session_id: i64, goal: &str, files_changed: &[String]) -> Result<()> {
    let path = zap_dir().join("session_log.md");
    let now = Utc::now().format("%Y-%m-%d").to_string();
    let files = if files_changed.is_empty() {
        "(no files modified)".to_string()
    } else {
        let mut seen = std::collections::HashSet::new();
        files_changed.iter()
            .filter(|f| seen.insert(*f))
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    };
    let entry = format!("## Session #{session_id} — {now}\nGoal: {goal}\nFiles: {files}\n\n");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let combined = format!("{}{}", entry, existing);
    // Cap at ~20k chars so the file doesn't grow unbounded
    let capped: String = combined.chars().take(20_000).collect();
    std::fs::write(&path, capped)?;
    Ok(())
}

// ── session_log.md ───────────────────────────────────────────────────────────

/// Load recent entries from `.zap/session_log.md`, capped at `max_chars`.
pub fn load_session_log(max_chars: usize) -> Option<String> {
    let s = std::fs::read_to_string(PathBuf::from(".zap").join("session_log.md")).ok()?;
    if s.trim().is_empty() {
        return None;
    }
    if s.len() <= max_chars {
        Some(s)
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        Some(format!("{}\n\n[… truncated]", truncated))
    }
}

// ── understanding.md ──────────────────────────────────────────────────────────

/// Load `.zap/understanding.md`, capped at `max_chars` for system-prompt injection.
pub fn load_understanding(max_chars: usize) -> Option<String> {
    let s = std::fs::read_to_string(PathBuf::from(".zap").join("understanding.md")).ok()?;
    if s.trim().is_empty() {
        return None;
    }
    if s.len() <= max_chars {
        Some(s)
    } else {
        let truncated: String = s.chars().take(max_chars).collect();
        Some(format!("{}\n\n[… truncated — see .zap/understanding.md for full content]", truncated))
    }
}

pub fn save_understanding(content: &str) -> Result<()> {
    std::fs::write(zap_dir().join("understanding.md"), content)?;
    Ok(())
}

/// Create a default `.zap/understanding.md` if it doesn't already exist,
/// or if it contains the auto-created placeholder text (meaning /init never
/// ran the LLM analysis). When index stats are available, fills in project
/// structure from the code index deterministically.
pub fn ensure_understanding_md(
    cwd_name: Option<String>,
    files: usize,
    symbols: usize,
    lang_counts: &[(String, usize)],
) -> Result<()> {
    let path = zap_dir().join("understanding.md");
    let is_placeholder = match std::fs::read_to_string(&path) {
        Ok(s) => s.contains("This project has not yet been analysed"),
        Err(_) => true, // doesn't exist → treat as placeholder
    };
    if path.exists() && !is_placeholder {
        return Ok(()); // has real content — don't touch
    }

    let content = if let Some(name) = cwd_name {
        let langs = if lang_counts.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = lang_counts.iter()
                .map(|(l, n)| format!("  - {l}: {n} symbols"))
                .collect();
            format!("\n## Languages\n{}\n", parts.join("\n"))
        };
        format!("\
# Understanding

Auto-generated from code index. Run `/init` for a detailed LLM-powered analysis.

## Project
{files} files · {symbols} symbols indexed · root: {name}
{langs}
## Structure
<!-- File-by-file analysis not yet run. Use /init to generate one. -->
"
        )
    } else {
        "\
# Understanding

<!-- This file was auto-created by zap. Run `/init` or manually edit it to add project-specific knowledge. -->

## Overview

*This project has not yet been analysed. Run `/init` to generate a full understanding based on the code index.*
"
        .to_string()
    };

    std::fs::write(&path, content)?;
    Ok(())
}
