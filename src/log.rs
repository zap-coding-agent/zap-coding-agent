/// Zap error/warning logger — writes to stdout AND ~/.zap/zap.log.
///
/// Use `zap_warn!` / `zap_error!` instead of `tracing::warn!` / `eprintln!`
/// for anything the user should be able to see or diagnose later.
///
/// LLM request/response logging goes to ~/.zap/llm.log via `write_llm()`.
use chrono::Utc;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::Mutex;

static WRITER: Mutex<Option<BufWriter<File>>> = Mutex::new(None);

pub fn log_path() -> std::path::PathBuf {
    let base = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".zap");
    std::fs::create_dir_all(&base).ok();
    base.join("zap.log")
}

pub fn llm_log_path() -> std::path::PathBuf {
    let base = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".zap");
    std::fs::create_dir_all(&base).ok();
    base.join("llm.log")
}

fn llm_requests_dir() -> std::path::PathBuf {
    let base = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".zap")
        .join("llm_requests");
    std::fs::create_dir_all(&base).ok();
    base
}

/// Save `body` (compact JSON, stream:false) to ~/.zap/llm_requests/<ts>_<slug>.json.
/// Returns the path so the caller can reference it in a curl command.
pub fn save_request_body(slug: &str, body: &str) -> Option<std::path::PathBuf> {
    let ts = Utc::now().format("%Y%m%d_%H%M%S%3f"); // millisecond precision avoids collisions
    let path = llm_requests_dir().join(format!("{}_{}.json", ts, slug));
    std::fs::write(&path, body).ok()?;
    Some(path)
}

/// Rotate log files at startup: trim `llm.log` to the last 24 h of entries,
/// and delete `llm_requests/` files older than 24 h.
/// Runs in a detached OS thread so it never blocks startup.
pub fn rotate_logs() {
    std::thread::spawn(|| {
        let cutoff = Utc::now() - chrono::Duration::hours(24);
        trim_llm_log(cutoff);
        trim_llm_requests(cutoff);
    });
}

fn trim_llm_log(cutoff: chrono::DateTime<Utc>) {
    use std::io::{BufRead, Seek, SeekFrom};

    let path = llm_log_path();
    if !path.exists() { return; }

    // Phase 1: scan for the byte offset of the first entry within the cutoff.
    // Each entry is preceded by a blank line, then "=== TIMESTAMP DIRECTION ===".
    let start: u64 = {
        let Ok(file) = std::fs::File::open(&path) else { return };
        let mut reader = std::io::BufReader::new(file);
        let mut keep_from: Option<u64> = None;
        let mut pos: u64 = 0;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Some(rest) = line.strip_prefix("=== ") {
                        if let Some(sp) = rest.find(' ') {
                            if let Ok(ts) = rest[..sp].parse::<chrono::DateTime<Utc>>() {
                                if ts >= cutoff {
                                    // Keep the preceding blank separator line too.
                                    keep_from = Some(pos.saturating_sub(1));
                                    break;
                                }
                            }
                        }
                    }
                    pos += n as u64;
                }
            }
        }
        match keep_from {
            None => {
                // All content is older than the cutoff — clear the file.
                let _ = std::fs::write(&path, "");
                return;
            }
            Some(0) => return, // file starts within the window, nothing to trim
            Some(s) => s,
        }
    };

    // Phase 2: stream-copy from `start` to a temp file, then atomically replace.
    let Ok(mut src) = std::fs::File::open(&path) else { return };
    if src.seek(SeekFrom::Start(start)).is_err() { return; }
    let tmp = path.with_extension("tmp");
    let ok = (|| -> std::io::Result<()> {
        let mut dst = std::fs::File::create(&tmp)?;
        std::io::copy(&mut src, &mut dst)?;
        dst.flush()
    })().is_ok();
    if ok {
        let _ = std::fs::rename(&tmp, &path);
    } else {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn trim_llm_requests(cutoff: chrono::DateTime<Utc>) {
    let dir = llm_requests_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let cutoff_sys = std::time::SystemTime::UNIX_EPOCH
        + std::time::Duration::from_secs(cutoff.timestamp().max(0) as u64);
    for entry in entries.flatten() {
        if let Ok(meta) = entry.metadata() {
            if let Ok(mtime) = meta.modified() {
                if mtime < cutoff_sys {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
}

/// Append one LLM direction block to ~/.zap/llm.log.
/// `direction` is e.g. "REQUEST [anthropic]" or "RESPONSE [anthropic]".
/// `payload`   is the pretty-printed JSON string (plus any curl block appended by the caller).
pub fn write_llm(direction: &str, payload: &str) {
    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let path = llm_log_path();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "\n=== {ts} {direction} ===");
        let _ = writeln!(f, "{payload}");
    }
}

pub fn write(level: &str, msg: &str) {
    let ts  = Utc::now().format("%Y-%m-%dT%H:%M:%S");
    let line = format!("[{}] {} {}", ts, level, msg);

    // Skip raw println! in TUI mode — background thread writes race with ratatui
    // rendering and land at whatever cursor position the last render left behind,
    // causing visible text overlap across panel boundaries.
    if !crate::tui::channel::is_tui_mode() {
        println!("  {}", line);
    }

    // Route WARN/ERROR into the chat so the user sees them even in TUI mode.
    if level.trim() == "WARN" || level.trim() == "ERROR" {
        crate::tui::channel::tui_send(
            crate::tui::channel::TuiEvent::LlmChunk(format!("\n⚠ {}\n", msg))
        );
    }

    // Also append to log file.
    let mut guard = WRITER.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        if let Ok(f) = OpenOptions::new().create(true).append(true).open(log_path()) {
            *guard = Some(BufWriter::new(f));
        }
    }
    if let Some(w) = guard.as_mut() {
        let _ = writeln!(w, "{}", line);
        let _ = w.flush();
    }
}

#[macro_export]
macro_rules! zap_warn {
    ($($arg:tt)*) => { $crate::log::write("WARN ", &format!($($arg)*)) };
}

#[macro_export]
macro_rules! zap_error {
    ($($arg:tt)*) => { $crate::log::write("ERROR", &format!($($arg)*)) };
}
