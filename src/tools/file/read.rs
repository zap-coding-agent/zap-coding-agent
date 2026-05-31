use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::tools::Tool;
use super::guard_path;

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }
    fn description(&self) -> &str {
        "Read a file's contents, with optional line range. \
         Output uses 'line | content' format with 1-based line numbers and a pipe \
         delimiter, so whitespace in the content is never ambiguous. \
         For large files, use offset + limit to read only the relevant section."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":   { "type": "string", "description": "Path to the file to read." },
                "offset": { "type": "integer", "description": "First line to read (0-based, default 0)." },
                "limit":  { "type": "integer", "description": "Maximum number of lines to return." }
            },
            "required": ["path"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("read '{}'", input["path"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"]
            .as_str()
            .context("read_file: 'path' must be a string")?;
        guard_path(path)?;

        let raw = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("read_file: cannot read '{}'", path))?;

        let offset = input["offset"].as_u64().unwrap_or(0) as usize;
        let limit  = input["limit"].as_u64().map(|l| l as usize);

        let lines: Vec<&str> = raw.lines().collect();
        let total = lines.len();
        let start = offset.min(total);
        let end   = limit.map(|l| (start + l).min(total)).unwrap_or(total);

        if start == end {
            return Ok(format!("(file '{}' has {} lines; offset {} is past the end)", path, total, offset));
        }

        let out = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:<6} | {}", start + i + 1, line))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(out)
    }
}
