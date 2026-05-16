use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::Write;

pub const AUDIT_LOG_PATH: &str = "agent_audit.jsonl";

#[derive(Serialize)]
struct AuditRecord<'a> {
    timestamp: String,
    event: &'a str,
}

pub fn record(event: &str) -> Result<()> {
    let rec = AuditRecord {
        timestamp: Utc::now().to_rfc3339(),
        event,
    };
    let line = serde_json::to_string(&rec)?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(AUDIT_LOG_PATH)?;

    writeln!(file, "{}", line)?;
    tracing::debug!(event = %event, "audit record written");
    Ok(())
}
