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

// ── Write jail ──────────────────────────────────────────────────────────────────

/// Extra roots the file-write tools may write to (beyond the project root and the
/// system temp dir), populated from `config.allowed_paths` at startup.
static ALLOWED_WRITE_ROOTS: std::sync::OnceLock<Vec<std::path::PathBuf>> = std::sync::OnceLock::new();

/// Initialize the extra write roots from config. Call once at session startup.
/// Tilde (`~`) is expanded. Idempotent — only the first call wins.
pub fn init_allowed_write_roots(allowed_paths: &[String]) {
    let roots: Vec<std::path::PathBuf> = allowed_paths
        .iter()
        .map(|p| {
            if let Some(rest) = p.strip_prefix("~/") {
                dirs::home_dir().map(|h| h.join(rest)).unwrap_or_else(|| std::path::PathBuf::from(p))
            } else {
                std::path::PathBuf::from(p)
            }
        })
        .collect();
    let _ = ALLOWED_WRITE_ROOTS.set(roots);
}

/// Guard a path that is about to be **written**. Stricter than `guard_path`:
/// in addition to the credential denylist + symlink resolution, the resolved
/// target must live under the project root, the system temp dir, or a configured
/// `allowed_paths` root. The agent has no legitimate reason to write outside
/// those, and a confined write surface contains both confused-model mistakes and
/// prompt-injected overwrites.
pub(super) fn guard_write_path(path: &str) -> Result<()> {
    // Denylist + symlink resolution first.
    guard_path(path)?;

    let resolved = resolve_symlinks(&normalize_path(path));

    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.push(cwd);
    }
    roots.push(std::env::temp_dir());
    if let Some(extra) = ALLOWED_WRITE_ROOTS.get() {
        roots.extend(extra.iter().cloned());
    }

    let within = roots.iter().any(|r| {
        let canon = r.canonicalize().unwrap_or_else(|_| r.clone());
        resolved.starts_with(&canon) || resolved.starts_with(r)
    });

    if within {
        Ok(())
    } else {
        anyhow::bail!(
            "security: writing to '{}' is outside the project root, the system temp \
             directory, and any configured allowed_paths. Add the directory to \
             `allowed_paths` in ~/.agent.toml if this is intentional.",
            path
        )
    }
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

#[cfg(test)]
mod jail_tests {
    use super::*;

    #[test]
    fn write_allowed_inside_project_and_temp() {
        let cwd = std::env::current_dir().unwrap();
        let in_project = cwd.join("zap_jail_test.txt");
        assert!(guard_write_path(in_project.to_str().unwrap()).is_ok());
        let in_temp = std::env::temp_dir().join("zap_jail_test.txt");
        assert!(guard_write_path(in_temp.to_str().unwrap()).is_ok());
    }

    #[test]
    fn write_rejected_outside_project() {
        // Home dir is a parent of the repo, not under it, and not in the
        // credential denylist — the jail (not the denylist) must reject it.
        if let Some(home) = dirs::home_dir() {
            let outside = home.join("zap_jail_outside_write_test.txt");
            let res = guard_write_path(outside.to_str().unwrap());
            assert!(res.is_err(), "write outside project/temp must be rejected");
            assert!(res.unwrap_err().to_string().contains("outside the project"));
        }
    }

    #[test]
    fn write_rejected_for_credential_path() {
        // Denylist still applies to writes.
        if let Some(home) = dirs::home_dir() {
            let ssh = home.join(".ssh").join("authorized_keys");
            assert!(guard_write_path(ssh.to_str().unwrap()).is_err());
        }
    }
}
