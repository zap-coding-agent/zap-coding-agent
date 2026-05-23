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
