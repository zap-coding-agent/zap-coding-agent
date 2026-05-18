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
        "Spawn a focused sub-agent to work on an independent goal in parallel. \
         The sub-agent has its own message history, all tools, and full tool access. \
         Returns a structured result: summary, files changed, turns taken, tool calls used. \
         \n\
         WHEN TO USE:\n\
         - 2+ independent sub-tasks with no shared file writes\n\
         - Each sub-task is non-trivial (needs ≥1 tool call)\n\
         - Results can be synthesised without needing each other's intermediate state\n\
         \n\
         HOW TO USE:\n\
         1. Announce the parallel plan in your text before calling spawn_agent\n\
         2. Issue ALL spawn_agent calls in the SAME response — they execute in parallel\n\
         3. After they complete, synthesise results into a coherent reply\n\
         \n\
         DO NOT spawn for: sequential tasks, tiny tasks, tasks writing the same files."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "The specific, self-contained task for the sub-agent. \
                                    Be precise — include file names, function names, or \
                                    other details needed to complete the work without \
                                    needing to ask the parent for clarification."
                },
                "context": {
                    "type": "string",
                    "description": "Relevant context from the parent session: findings so far, \
                                    constraints, or decisions the sub-agent must respect. \
                                    Include anything the sub-agent can't discover on its own."
                },
                "files_in_scope": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of files this agent will read or write. \
                                    Used to detect conflicts when spawning multiple agents. \
                                    Example: [\"src/auth.rs\", \"src/auth/jwt.rs\"]"
                }
            },
            "required": ["goal"]
        })
    }

    fn permission_context(&self, input: &serde_json::Value) -> String {
        let goal = input["goal"].as_str().unwrap_or("?");
        let short = if goal.len() > 72 { &goal[..72] } else { goal };
        if let Some(files) = input["files_in_scope"].as_array() {
            let names: Vec<&str> = files.iter()
                .filter_map(|v| v.as_str())
                .take(4)
                .collect();
            let suffix = if files.len() > 4 { format!(" +{} more", files.len() - 4) } else { String::new() };
            format!("spawn sub-agent: {}  [{}{}]", short, names.join(", "), suffix)
        } else {
            format!("spawn sub-agent: {}", short)
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String> {
        let goal    = input["goal"].as_str().context("spawn_agent: 'goal' must be a string")?;
        let context = input["context"].as_str().unwrap_or("");

        let full_goal = if context.is_empty() {
            goal.to_string()
        } else {
            format!("{}\n\n## Context from parent agent\n{}", goal, context)
        };

        crate::agent_core::run_subagent(&full_goal, &self.config).await
    }
}
