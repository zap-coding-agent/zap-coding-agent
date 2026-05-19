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

/// Append one LLM direction block to ~/.zap/llm.log.
/// `direction` is e.g. "REQUEST [anthropic]" or "RESPONSE [anthropic]".
/// `payload`   is the pretty-printed JSON string.
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

    // Always visible on screen.
    println!("  {}", line);

    // In TUI mode println! writes to the alternate screen buffer and is hidden
    // by the next render. Route WARN/ERROR into the chat via LlmChunk so the
    // user actually sees them.
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
