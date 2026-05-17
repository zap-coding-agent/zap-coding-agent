use anyhow::{Context, Result};
use async_trait::async_trait;

use super::Tool;

// ── spawn_agent ───────────────────────────────────────────────────────────────

pub struct SpawnAgentTool {
    config: crate::config::Config,
}

impl SpawnAgentTool {
    pub fn new(config: crate::config::Config) -> Self {
        Self { config }
    }
}

#[async_trait]
impl Tool for SpawnAgentTool {
    fn name(&self) -> &str { "spawn_agent" }
    fn description(&self) -> &str {
        "Spawn a sub-agent to work on a focused goal, independently and in parallel. \
         The sub-agent has its own message history and access to all tools. \
         Returns the sub-agent's final response. \
         Use this to parallelise independent tasks — e.g. analyse multiple files, \
         implement two separate features at once, or run tests while writing code."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "The task for the sub-agent to accomplish. Be specific and self-contained."
                },
                "context": {
                    "type": "string",
                    "description": "Optional additional context or constraints for the sub-agent."
                }
            },
            "required": ["goal"]
        })
    }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("spawn sub-agent: {}", input["goal"].as_str().unwrap_or("?"))
    }
    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let goal    = input["goal"].as_str().context("spawn_agent: 'goal' must be a string")?;
        let context = input["context"].as_str().unwrap_or("");

        let full_goal = if context.is_empty() {
            goal.to_string()
        } else {
            format!("{}\n\nAdditional context: {}", goal, context)
        };

        crate::agent_core::run_subagent(&full_goal, &self.config).await
    }
}
