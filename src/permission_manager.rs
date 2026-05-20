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

    /// TUI-native permission prompt — renders at the bottom of the screen while
    /// raw mode stays active. Called with the alternate screen still open.
    fn prompt_batch_tui(
        &mut self,
        pending: &[(String, String, String)],
    ) -> Result<Vec<bool>> {
        use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
        use crossterm::{cursor, execute, terminal};
        use std::time::Duration;

        // Build the dialog box.
        let width: usize = 62;
        let bar = "─".repeat(width - 2);
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("┌{}┐", bar));

        if pending.len() == 1 {
            let (_, name, ctx) = &pending[0];
            lines.push(format!("│  Tool : {:<51} │", &name[..name.len().min(51)]));
            if !ctx.is_empty() {
                let s = if ctx.chars().count() > 51 {
                    format!("{}…", ctx.chars().take(50).collect::<String>())
                } else {
                    ctx.clone()
                };
                lines.push(format!("│  What : {:<51} │", s));
            }
        } else {
            lines.push(format!("│  Agent wants to run {} operation(s):{:<25} │", pending.len(), ""));
            for (_, name, ctx) in pending.iter().take(5) {
                let s = if ctx.chars().count() > 38 {
                    format!("{}…", ctx.chars().take(37).collect::<String>())
                } else {
                    ctx.clone()
                };
                lines.push(format!("│    · {:<12}  {:<38} │", name, s));
            }
            if pending.len() > 5 {
                lines.push(format!("│    … and {} more{:<44} │", pending.len() - 5, ""));
            }
        }

        lines.push(format!("│{:<width$}│", "", width = width - 2));
        lines.push(format!("│  [Y] Allow   [N] Deny   [A] Always allow{:<21}│", ""));
        lines.push(format!("└{}┘", bar));

        // Position at the bottom of the terminal, clear that area, print.
        let (_, rows) = terminal::size().unwrap_or((80, 24));
        let dialog_h = lines.len() as u16;
        let start_row = rows.saturating_sub(dialog_h + 1);

        execute!(
            io::stdout(),
            cursor::MoveTo(0, start_row),
            terminal::Clear(terminal::ClearType::FromCursorDown),
        )?;
        for line in &lines {
            print!("\r{}\r\n", line);
        }
        io::stdout().flush()?;

        // Read a single keypress — raw mode is still active, no competition.
        let decision = loop {
            if event::poll(Duration::from_millis(200))? {
                if let Event::Key(key) = event::read()? {
                    // Skip Release events (Windows fires Press+Release per key).
                    if key.kind == KeyEventKind::Release { continue; }
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

        // Show brief feedback in the same area, then clear (TUI will repaint).
        let msg = match decision {
            "a" => "  ✓ Always allowed",
            "y" => "  ✓ Allowed",
            _   => "  ✗ Denied",
        };
        execute!(
            io::stdout(),
            cursor::MoveTo(0, start_row),
            terminal::Clear(terminal::ClearType::FromCursorDown),
        )?;
        print!("\r{}\r\n", msg);
        io::stdout().flush()?;

        if decision == "a" {
            for (_, name, _) in pending {
                for related in tool_grant_class(name) {
                    self.session_grants.insert(related.to_string(), true);
                }
            }
        }

        tracing::info!(allowed, count = pending.len(), "TUI batch permission decision");
        Ok(vec![allowed; pending.len()])
    }
}
