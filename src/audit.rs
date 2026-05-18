use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::Mutex;

pub fn audit_log_path() -> std::path::PathBuf {
    let base = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".zap");
    std::fs::create_dir_all(&base).ok();
    base.join("audit.jsonl")
}

#[derive(Serialize)]
struct AuditRecord<'a> {
    timestamp: String,
    event: &'a str,
}

static WRITER: Mutex<Option<BufWriter<File>>> = Mutex::new(None);

pub fn record(event: &str) -> Result<()> {
    let rec = AuditRecord {
        timestamp: Utc::now().to_rfc3339(),
        event,
    };
    let line = serde_json::to_string(&rec)?;

    let mut guard = WRITER.lock()
        .map_err(|_| anyhow::anyhow!("audit mutex poisoned"))?;
    if guard.is_none() {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(audit_log_path())?;
        *guard = Some(BufWriter::new(file));
    }
    let writer = guard.as_mut()
        .ok_or_else(|| anyhow::anyhow!("audit writer unavailable"))?;
    writeln!(writer, "{}", line)?;
    writer.flush()?;

    tracing::debug!(event = %event, "audit record written");
    Ok(())
}
