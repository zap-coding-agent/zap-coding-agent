use crate::config::PermissionMode;
use anyhow::Result;
use std::collections::HashMap;
use std::io::{self, Write};

/// Tools that require explicit user approval in Ask mode.
/// Read-only and network-read tools are auto-allowed to avoid prompt fatigue.
const WRITE_TOOLS: &[&str] = &["shell", "write_file", "edit_file"];

pub struct PermissionManager {
    pub mode: PermissionMode,
    /// Per-session "always allow" grants so the user isn't re-prompted.
    session_grants: HashMap<String, bool>,
}

impl PermissionManager {
    pub fn new(mode: PermissionMode) -> Self {
        Self { mode, session_grants: HashMap::new() }
    }

    /// Returns true if the tool is allowed to execute.
    pub fn check(&mut self, tool_name: &str, context: &str) -> Result<bool> {
        match self.mode {
            PermissionMode::Auto => Ok(true),

            PermissionMode::Deny => {
                tracing::warn!(tool = %tool_name, "tool denied by policy");
                Ok(false)
            }

            PermissionMode::Ask => {
                if !WRITE_TOOLS.contains(&tool_name) {
                    tracing::debug!(tool = %tool_name, "auto-allowed (read-only)");
                    return Ok(true);
                }

                // Check per-session grant (user said "always" earlier this session).
                if let Some(&granted) = self.session_grants.get(tool_name) {
                    tracing::debug!(tool = %tool_name, "session-grant applied");
                    return Ok(granted);
                }

                println!();
                println!("  Tool : {}", tool_name);
                if !context.is_empty() {
                    println!("  What : {}", context);
                }
                print!("  Allow? [y/N/a(lways)] ");
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let decision = input.trim().to_lowercase();
                let allowed = matches!(decision.as_str(), "y" | "yes" | "a" | "always");

                if matches!(decision.as_str(), "a" | "always") {
                    self.session_grants.insert(tool_name.to_string(), true);
                    println!("  → will always allow '{}' for this session.", tool_name);
                }

                tracing::info!(tool = %tool_name, allowed, "user permission decision");
                Ok(allowed)
            }
        }
    }
}
