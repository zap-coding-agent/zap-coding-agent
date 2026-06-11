use anyhow::{Context, Result};
use async_trait::async_trait;

use crate::tools::Tool;
use super::{guard_write_path, normalize_path};

pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str { "write_file" }
    fn description(&self) -> &str {
        "Write content to a file, creating it or overwriting it. \
         Requires user approval before executing."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path":    { "type": "string", "description": "Destination file path." },
                "content": { "type": "string", "description": "Content to write." }
            },
            "required": ["path", "content"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        let path = input["path"].as_str().unwrap_or("?");
        let bytes = input["content"].as_str().map(|s| s.len()).unwrap_or(0);
        format!("write {} bytes to '{}'", bytes, path)
    }
    fn affected_path<'a>(&self, input: &'a serde_json::Value) -> Option<&'a str> {
        input["path"].as_str()
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"]
            .as_str()
            .context("write_file: 'path' must be a string")?;
        guard_write_path(path)?;
        let content = input["content"]
            .as_str()
            .context("write_file: 'content' must be a string")?;

        let _ = crate::snapshot::save_snapshot(path);

        let abs_path = normalize_path(path);
        if let Some(parent) = abs_path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!(
                        "write_file: cannot create dirs '{}' (resolved to '{}')",
                        path, parent.to_string_lossy().replace('\\', "/")
                    ))?;
            }
        }

        tokio::fs::write(&abs_path, content)
            .await
            .with_context(|| format!(
                "write_file: cannot write '{}' (resolved to '{}')",
                path, abs_path.to_string_lossy().replace('\\', "/")
            ))?;

        let abs_display = abs_path.to_string_lossy().replace('\\', "/");
        Ok(format!("wrote {} bytes to '{}' ({})", content.len(), path, abs_display))
    }
}
