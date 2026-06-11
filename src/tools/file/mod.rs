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

/// Resolve symlinks on a path that may not fully exist yet.
///
/// `normalize_path` only does lexical `.`/`..` cleanup — it does NOT follow
/// symlinks, so a link inside the project pointing at `~/.ssh/id_rsa` would
/// slip past a substring guard. This walks up to the nearest existing ancestor,
/// canonicalizes it (which resolves every symlink in that prefix), then
/// re-appends the not-yet-existing tail. The result is the real on-disk target
/// the OS would open, which is what the guard must inspect.
pub(super) fn resolve_symlinks(abs: &std::path::Path) -> std::path::PathBuf {
    let mut existing = abs.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if existing.exists() {
            if let Ok(canon) = existing.canonicalize() {
                let mut out = canon;
                for comp in tail.iter().rev() {
                    out.push(comp);
                }
                return out;
            }
            break;
        }
        match existing.file_name() {
            Some(name) => {
                tail.push(name.to_os_string());
                if !existing.pop() {
                    break;
                }
            }
            None => break,
        }
    }
    abs.to_path_buf()
}

/// Reject paths that point at known-sensitive locations (credentials, keys, config).
/// Called before every file read or write.
///
/// The path is symlink-resolved first (see `resolve_symlinks`) so a link inside
/// the project cannot be used to reach a blocked target.
pub(super) fn guard_path(path: &str) -> Result<()> {
    let abs = normalize_path(path);
    // Resolve symlinks BEFORE matching — a link's lexical path would otherwise
    // hide the real (blocked) destination.
    let resolved = resolve_symlinks(&abs);
    let abs_str = resolved.to_string_lossy().to_lowercase();

    // Credential / secret surface of a typical dev machine. This is a
    // defense-in-depth denylist, not a jail: zap is a coding agent that
    // legitimately reads arbitrary project files, temp files, and /dev/null,
    // so a hard allowlist would break normal use. These segments cover the
    // high-value credential stores an exfiltration attempt would target.
    const BLOCKED_SEGMENTS: &[&str] = &[
        // SSH / GPG / PKI
        "/.ssh/", "/.gnupg/", "id_rsa", "id_ed25519", "id_ecdsa", "id_dsa",
        // Cloud provider credentials
        "/.aws/", "/.azure/", "/.config/gcloud", "/.kube/", "/.docker/",
        "/.config/containers/auth.json", "/.oci/",
        // Git / VCS / package-registry tokens
        "/.git-credentials", "/.config/gh/", "/.config/git/credentials",
        "/.netrc", "/.npmrc", "/.pypirc", "/.cargo/credentials",
        "/.gem/credentials", "/.terraform.d/credentials",
        // Databases
        "/.pgpass", "/.my.cnf", "/.mylogin.cnf",
        // Shell / system
        "/.bash_history", "/.zsh_history",
        "/etc/passwd", "/etc/shadow", "/etc/sudoers",
        // zap's own config (API keys)
        "/.agent.toml",
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
