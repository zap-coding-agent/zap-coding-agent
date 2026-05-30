use anyhow::{Context, Result};
use async_trait::async_trait;

use super::Tool;

pub(super) struct UndoEditTool;

#[async_trait]
impl Tool for UndoEditTool {
    fn name(&self) -> &str { "undo_edit" }
    fn description(&self) -> &str {
        "Revert the most recent edit or write to a file, restoring its previous content. \
         Multiple undos are supported (one per previous edit). \
         Use 'list' as the path to see which files have snapshots available."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "File to undo edits on, or 'list' to see available snapshots." }
            },
            "required": ["path"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("undo edit on '{}'", input["path"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let path = input["path"].as_str().context("undo_edit: 'path' required")?;
        if path == "list" {
            let snaps = crate::snapshot::list_snapshots();
            if snaps.is_empty() {
                return Ok("No snapshots available (no edits made this session).".to_string());
            }
            return Ok(format!("Files with undo snapshots:\n{}", snaps.join("\n")));
        }
        let restored = crate::snapshot::restore_snapshot(path)?;
        Ok(format!(
            "Restored '{}' to previous version ({} bytes).",
            path, restored.len()
        ))
    }
}
