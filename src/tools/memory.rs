use anyhow::Result;
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};

use super::Tool;

/// Set to true by memory_set / memory_delete on a successful write.
/// The session's turn loop checks this flag after each tool round and patches
/// self.system with fresh memory so the next LLM call sees updated facts.
static MEMORY_DIRTY: AtomicBool = AtomicBool::new(false);

/// Atomically consume the dirty flag. Returns true if memory changed since last call.
pub fn take_dirty_flag() -> bool {
    MEMORY_DIRTY.swap(false, Ordering::Relaxed)
}

// ── memory_set ────────────────────────────────────────────────────────────────

pub struct MemorySetTool;

#[async_trait]
impl Tool for MemorySetTool {
    fn name(&self) -> &str { "memory_set" }

    fn description(&self) -> &str {
        "Persist a key-value fact that should survive across sessions. Call this \
         proactively — without being asked — when you observe something worth \
         remembering: a user preference, a project convention, a recurring team rule, \
         a useful endpoint, or any fact that would improve future sessions. \
         Key should be short and descriptive (e.g. 'preferred_pr_style', \
         'test_command', 'deploy_target'). Facts are stored globally and injected \
         into the system prompt of every future session."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key":   { "type": "string", "description": "Short descriptive identifier for the fact" },
                "value": { "type": "string", "description": "The fact to remember" }
            },
            "required": ["key", "value"]
        })
    }

    fn permission_context(&self, input: &serde_json::Value) -> String {
        let key   = input["key"].as_str().unwrap_or("?");
        let value = input["value"].as_str().unwrap_or("?");
        format!("{} = {}", key, value)
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let key   = input["key"].as_str()
            .ok_or_else(|| anyhow::anyhow!("key is required"))?;
        let value = input["value"].as_str()
            .ok_or_else(|| anyhow::anyhow!("value is required"))?;

        crate::persistence::init()?.set_memory(key, value)?;
        MEMORY_DIRTY.store(true, Ordering::Relaxed);
        Ok(format!("Saved: {} = {}", key, value))
    }
}

// ── memory_delete ─────────────────────────────────────────────────────────────

pub struct MemoryDeleteTool;

#[async_trait]
impl Tool for MemoryDeleteTool {
    fn name(&self) -> &str { "memory_delete" }

    fn description(&self) -> &str {
        "Remove a previously saved memory fact by key. Use when a fact is stale, \
         incorrect, or no longer relevant (e.g. a preference changed, a project was \
         renamed, an endpoint moved). After deletion the fact is removed from future \
         session prompts immediately."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "key": { "type": "string", "description": "Key of the fact to delete" }
            },
            "required": ["key"]
        })
    }

    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("delete memory: {}", input["key"].as_str().unwrap_or("?"))
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let key = input["key"].as_str()
            .ok_or_else(|| anyhow::anyhow!("key is required"))?;

        crate::persistence::init()?.delete_memory(key)?;
        MEMORY_DIRTY.store(true, Ordering::Relaxed);
        Ok(format!("Deleted memory key: {}", key))
    }
}
