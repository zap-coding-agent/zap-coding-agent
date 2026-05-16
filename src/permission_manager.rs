use crate::config::PermissionMode;
use anyhow::Result;
use std::io::{self, Write};

/// Tools that always require explicit user approval in Ask mode.
/// Read-only tools are auto-allowed in Ask mode to avoid prompt fatigue.
const WRITE_TOOLS: &[&str] = &["shell", "write_file", "edit_file"];

pub struct PermissionManager {
    pub mode: PermissionMode,
}

impl PermissionManager {
    pub fn new(mode: PermissionMode) -> Self {
        Self { mode }
    }

    /// Returns true if the tool is allowed to execute.
    /// `context` is a one-line human-readable description of what will happen
    /// (filled in by each tool; added properly in Task 7.3).
    pub fn check(&self, tool_name: &str, context: &str) -> Result<bool> {
        match self.mode {
            PermissionMode::Auto => Ok(true),

            PermissionMode::Deny => {
                tracing::warn!(tool = %tool_name, "tool denied by policy");
                Ok(false)
            }

            PermissionMode::Ask => {
                // Read-only tools never interrupt the user.
                if !WRITE_TOOLS.contains(&tool_name) {
                    tracing::debug!(tool = %tool_name, "auto-allowed (read-only)");
                    return Ok(true);
                }

                println!();
                println!("  Tool : {}", tool_name);
                if !context.is_empty() {
                    println!("  What : {}", context);
                }
                print!("Allow? [y/N] ");
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let allowed = matches!(input.trim().to_lowercase().as_str(), "y" | "yes");
                tracing::info!(tool = %tool_name, allowed, "user permission decision");
                Ok(allowed)
            }
        }
    }
}
