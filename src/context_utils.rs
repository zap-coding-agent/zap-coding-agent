//! Pure utility helpers used by context_manager.

pub(crate) fn strip_frontmatter(raw: &str) -> &str {
    let s = raw.trim_start();
    if !s.starts_with("---") { return s; }
    let after = &s[3..];
    if let Some(pos) = after.find("\n---") {
        after[pos + 4..].trim_start_matches('\n')
    } else {
        s
    }
}

pub(crate) fn expand_tilde(p: &str) -> std::path::PathBuf {
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    std::path::PathBuf::from(p)
}

/// Return the user's home directory, checking $HOME, $USERPROFILE, $HOMEDRIVE+$HOMEPATH.
pub(crate) fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .or_else(|_| {
            let drive = std::env::var("HOMEDRIVE").unwrap_or_default();
            let path  = std::env::var("HOMEPATH").unwrap_or_default();
            if drive.is_empty() && path.is_empty() { Err(std::env::VarError::NotPresent) }
            else { Ok(format!("{}{}", drive, path)) }
        })
        .ok()
        .map(std::path::PathBuf::from)
}

pub(crate) fn git_status_summary() -> Option<String> {
    let mut child = std::process::Command::new("git")
        .args(["status", "--short"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                return None;
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }

    let out = child.wait_with_output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}
