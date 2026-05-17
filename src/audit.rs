use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::sync::Mutex;

pub const AUDIT_LOG_PATH: &str = "agent_audit.jsonl";

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

    let mut guard = WRITER.lock().unwrap();
    if guard.is_none() {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(AUDIT_LOG_PATH)?;
        *guard = Some(BufWriter::new(file));
    }
    let writer = guard.as_mut().unwrap();
    writeln!(writer, "{}", line)?;
    writer.flush()?;

    tracing::debug!(event = %event, "audit record written");
    Ok(())
}
