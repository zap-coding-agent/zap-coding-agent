pub(super) fn git_branch() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

pub(super) fn git_status() -> (bool, usize, usize) {
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);

    let (ahead, behind) = std::process::Command::new("git")
        .args(["rev-list", "--left-right", "--count", "HEAD...@{upstream}"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let output = String::from_utf8(o.stdout).ok()?;
                let parts: Vec<&str> = output.split_whitespace().collect();
                if parts.len() == 2 {
                    let ahead = parts[0].parse().ok()?;
                    let behind = parts[1].parse().ok()?;
                    return Some((ahead, behind));
                }
            }
            None
        })
        .unwrap_or((0, 0));

    (dirty, ahead, behind)
}

/// Returns " (+N/−M)" from `git diff HEAD --shortstat`, or "" if no changes.
pub(super) fn git_diff_shortstat() -> String {
    let out = std::process::Command::new("git")
        .args(["diff", "HEAD", "--shortstat"])
        .output()
        .ok();
    let Some(out) = out else { return String::new() };
    if !out.status.success() { return String::new(); }
    let text = String::from_utf8(out.stdout).unwrap_or_default();
    let mut added = 0usize;
    let mut removed = 0usize;
    for part in text.split(',') {
        let p = part.trim();
        if p.contains("insertion") {
            added = p.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        } else if p.contains("deletion") {
            removed = p.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
        }
    }
    if added == 0 && removed == 0 {
        String::new()
    } else {
        format!(" (+{}/−{})", added, removed)
    }
}
