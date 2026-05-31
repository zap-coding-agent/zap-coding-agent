use anyhow::Result;
use colored::Colorize;
use similar::{ChangeTag, TextDiff};

mod read;
mod edit;
mod write;
mod glob;

pub(super) use read::ReadFileTool;
pub(super) use edit::{EditFileTool, BatchEditTool};
pub(super) use write::WriteFileTool;
pub(super) use glob::GlobReadTool;

// ── Path safety guard ─────────────────────────────────────────────────────────

/// Normalize a path string to an absolute path without requiring it to exist
/// (unlike std::fs::canonicalize). Resolves `.` and `..` components.
pub(super) fn normalize_path(path: &str) -> std::path::PathBuf {
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
pub(super) fn guard_path(path: &str) -> Result<()> {
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

// ── shared diff printer ────────────────────────────────────────────────────────

pub(super) fn print_diff(before: &str, after: &str) {
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
