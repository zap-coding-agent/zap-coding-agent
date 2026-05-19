use crate::config::PermissionMode;
use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;
use std::io::{self, Write};

/// Tools that require explicit user approval in Ask mode.
const WRITE_TOOLS: &[&str] = &["shell", "write_file", "edit_file", "batch_edit", "spawn_agent"];

/// Related tools that share an "always" grant.
/// Granting one member grants all members — avoids re-prompting for semantically
/// identical operations (e.g. edit_file vs write_file vs batch_edit).
fn tool_grant_class(name: &str) -> &'static [&'static str] {
    match name {
        "edit_file" | "write_file" | "batch_edit" | "undo_edit" => {
            &["edit_file", "write_file", "batch_edit", "undo_edit"]
        }
        "shell" => &["shell"],
        _ => &[],
    }
}

pub enum QuickDecision { Allow, Deny, NeedsPrompt }

pub struct PermissionManager {
    pub mode: PermissionMode,
    /// Per-session grants; keyed by tool name, set true by "always".
    session_grants: HashMap<String, bool>,
}

impl PermissionManager {
    pub fn new(mode: PermissionMode) -> Self {
        Self { mode, session_grants: HashMap::new() }
    }

    /// Non-blocking check — returns whether to allow, deny, or ask the user.
    pub fn quick_check(&self, tool_name: &str) -> QuickDecision {
        match self.mode {
            PermissionMode::Auto => QuickDecision::Allow,
            PermissionMode::Deny => QuickDecision::Deny,
            PermissionMode::Ask  => {
                if !WRITE_TOOLS.contains(&tool_name) { return QuickDecision::Allow; }
                if self.session_grants.get(tool_name).copied() == Some(true) {
                    return QuickDecision::Allow;
                }
                QuickDecision::NeedsPrompt
            }
        }
    }

    /// Show ONE grouped prompt for all calls that need user input.
    ///
    /// `pending` is a slice of `(id, tool_name, context_string)`.
    /// Returns a parallel `Vec<bool>` — `true` = approved, `false` = denied.
    pub fn prompt_batch(
        &mut self,
        pending: &[(String, String, String)],
    ) -> Result<Vec<bool>> {
        if pending.is_empty() { return Ok(vec![]); }

        // In TUI mode, use a native TUI dialog instead of breaking out to CLI
        if crate::tui::channel::is_tui_mode() {
            return self.prompt_batch_tui(pending);
        }

        // CLI mode: use the existing prompt
        println!();
        if pending.len() == 1 {
            let (_, name, ctx) = &pending[0];
            println!("  {} {}", "Tool:".truecolor(100, 95, 130), name.truecolor(100, 210, 255).bold());
            if !ctx.is_empty() {
                println!("  {} {}", "What:".truecolor(100, 95, 130), ctx.truecolor(130, 125, 150));
            }
        } else {
            println!(
                "  {} {} operation(s):",
                "Agent wants to run".truecolor(100, 95, 130),
                pending.len().to_string().cyan().bold(),
            );
            for (_, name, ctx) in pending {
                let ctx_disp = if ctx.chars().count() > 55 {
                    format!("{}…", ctx.chars().take(54).collect::<String>())
                } else { ctx.clone() };
                println!(
                    "    {} {}  {}",
                    "·".truecolor(70, 65, 90),
                    format!("{:<12}", name).truecolor(100, 210, 255).bold(),
                    ctx_disp.truecolor(130, 125, 150),
                );
            }
        }

        print!("  Allow? [y/N/a(lways)] ");
        io::stdout().flush()?;

        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;

        let dec = buf.trim().to_lowercase();
        let allowed = matches!(dec.as_str(), "y" | "yes" | "a" | "always");

        if matches!(dec.as_str(), "a" | "always") {
            for (_, name, _) in pending {
                for related in tool_grant_class(name) {
                    self.session_grants.insert(related.to_string(), true);
                }
            }
            println!(
                "  {} auto-approved for the rest of this session.",
                "→".truecolor(100, 210, 255)
            );
        }

        tracing::info!(allowed, count = pending.len(), "batch permission decision");
        Ok(vec![allowed; pending.len()])
    }

    /// TUI-native permission prompt that stays within the TUI interface.
    fn prompt_batch_tui(
        &mut self,
        pending: &[(String, String, String)],
    ) -> Result<Vec<bool>> {
        use crossterm::event::{self, Event, KeyCode, KeyModifiers};
        use std::time::Duration;

        // Build the prompt message
        let mut prompt_lines = Vec::new();
        prompt_lines.push("┌─ Permission Required ─────────────────────────────────────┐".to_string());
        
        if pending.len() == 1 {
            let (_, name, ctx) = &pending[0];
            prompt_lines.push(format!("│ Tool: {}                                                  │", name));
            if !ctx.is_empty() {
                let ctx_short = if ctx.len() > 54 { format!("{}…", &ctx[..53]) } else { ctx.clone() };
                prompt_lines.push(format!("│ What: {:<54} │", ctx_short));
            }
        } else {
            prompt_lines.push(format!("│ Agent wants to run {} operations:                        │", pending.len()));
            for (_, name, ctx) in pending.iter().take(5) {
                let ctx_short = if ctx.len() > 40 { format!("{}…", &ctx[..39]) } else { ctx.clone() };
                prompt_lines.push(format!("│   · {:<12} {:<40} │", name, ctx_short));
            }
            if pending.len() > 5 {
                prompt_lines.push(format!("│   ... and {} more                                         │", pending.len() - 5));
            }
        }
        
        prompt_lines.push("│                                                            │".to_string());
        prompt_lines.push("│ [Y] Allow    [N] Deny    [A] Always allow                 │".to_string());
        prompt_lines.push("└────────────────────────────────────────────────────────────┘".to_string());

        // Print the prompt
        println!("\n{}", prompt_lines.join("\n"));
        io::stdout().flush()?;

        // Wait for user input
        let decision = loop {
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => break "y",
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => break "n",
                        KeyCode::Char('a') | KeyCode::Char('A') => break "a",
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break "n",
                        _ => {}
                    }
                }
            }
        };

        let allowed = matches!(decision, "y" | "a");

        if decision == "a" {
            for (_, name, _) in pending {
                for related in tool_grant_class(name) {
                    self.session_grants.insert(related.to_string(), true);
                }
            }
            println!("  → Auto-approved for the rest of this session.\n");
        } else if allowed {
            println!("  → Allowed\n");
        } else {
            println!("  → Denied\n");
        }

        io::stdout().flush()?;
        std::thread::sleep(Duration::from_millis(500)); // Brief pause so user sees the result

        tracing::info!(allowed, count = pending.len(), "TUI batch permission decision");
        Ok(vec![allowed; pending.len()])
    }
}
