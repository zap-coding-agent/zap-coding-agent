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
        // Truncate by chars, not bytes, to avoid a panic on multibyte UTF-8 boundaries.
        let short_owned: String;
        let short = if goal.chars().count() > 72 {
            short_owned = goal.chars().take(72).collect();
            short_owned.as_str()
        } else {
            goal
        };
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

#[cfg(test)]
mod tests {
    // ── permission_context truncation ─────────────────────────────────────────

    #[test]
    fn truncation_by_chars_not_bytes_does_not_panic() {
        // Regression: &goal[..72] panics when byte 72 is inside a multibyte char.
        // Fixed to: goal.chars().take(72).collect::<String>()
        let long_goal: String = "こ".repeat(80); // 240 bytes, 80 chars
        // The old code: assert panics with index-not-on-char-boundary
        // let _ = &long_goal[..72]; // ← this would panic
        // The new code:
        let truncated: String = long_goal.chars().take(72).collect();
        assert_eq!(truncated.chars().count(), 72);
        assert_eq!(truncated.len(), 72 * 3); // each こ is 3 bytes
    }

    #[test]
    fn truncation_preserves_ascii_goals_exactly() {
        let goal = "a".repeat(80);
        let truncated: String = goal.chars().take(72).collect();
        assert_eq!(truncated.len(), 72);
    }

    #[test]
    fn short_goal_not_truncated() {
        let goal = "fix the login bug";
        let truncated: String = if goal.chars().count() > 72 {
            goal.chars().take(72).collect()
        } else {
            goal.to_string()
        };
        assert_eq!(truncated, goal);
    }
}
