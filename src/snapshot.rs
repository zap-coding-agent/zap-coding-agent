/// File snapshot system for undo support.
///
/// Before every `edit_file` or `write_file`, the previous content is saved
/// to `~/.zap/snapshots/` keyed by file path.  `undo_edit` restores the
/// most recent snapshot.
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

/// In-memory snapshot store (one per session, not persisted across restarts).
/// Key = canonical path, Value = stack of previous contents (most recent last).
static SNAPSHOTS: std::sync::LazyLock<Mutex<HashMap<PathBuf, Vec<String>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Save the current content of `path` before modifying it.
/// Call this *before* writing to the file.
pub fn save_snapshot(path: &str) -> Result<()> {
    let canonical = std::fs::canonicalize(path)
        .unwrap_or_else(|_| PathBuf::from(path));

    // Only snapshot if the file already exists.
    if !canonical.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&canonical)
        .with_context(|| format!("snapshot: cannot read '{}'", path))?;

    let mut map = SNAPSHOTS.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(canonical).or_default().push(content);
    Ok(())
}

/// Restore the most recent snapshot for `path`.
/// Returns the restored content, or an error if no snapshot exists.
pub fn restore_snapshot(path: &str) -> Result<String> {
    let canonical = std::fs::canonicalize(path)
        .unwrap_or_else(|_| PathBuf::from(path));

    let mut map = SNAPSHOTS.lock().unwrap_or_else(|e| e.into_inner());
    let stack = map.get_mut(&canonical)
        .context(format!("undo: no snapshot found for '{}'", path))?;

    let content = stack.pop()
        .context(format!("undo: no more snapshots for '{}'", path))?;

    // Clean up empty stacks.
    if stack.is_empty() {
        map.remove(&canonical);
    }

    // Write the old content back.
    std::fs::write(&canonical, &content)
        .with_context(|| format!("undo: cannot write '{}'", path))?;

    Ok(content)
}

/// List all files that have snapshots available.
pub fn list_snapshots() -> Vec<String> {
    let map = SNAPSHOTS.lock().unwrap_or_else(|e| e.into_inner());
    map.iter()
        .filter(|(_, stack)| !stack.is_empty())
        .map(|(path, stack)| format!("{} ({} undo(s))", path.display(), stack.len()))
        .collect()
}

/// Return (path, before, after) for every file edited this session.
/// `before` = content before the first edit; `after` = current on-disk content.
/// Used by the TUI diff viewer when git is unavailable (non-git directories).
pub fn snapshot_diffs() -> Vec<(PathBuf, String, String)> {
    let map = SNAPSHOTS.lock().unwrap_or_else(|e| e.into_inner());
    map.iter()
        .filter(|(_, stack)| !stack.is_empty())
        .filter_map(|(path, stack)| {
            let before = stack[0].clone(); // original before any edits
            let after = std::fs::read_to_string(path).ok()?;
            Some((path.clone(), before, after))
        })
        .collect()
}
