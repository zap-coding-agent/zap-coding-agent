use colored::Colorize;
use inquire::Select;
use crate::{
    audit,
    config::{Config, PermissionMode},
    llm_client::{ContentBlock, Message},
};
use super::super::Session;

/// Truncate a string to at most `max` characters, adding "…" if shortened.
fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn print_conversation(messages: &[Message]) {
    if messages.is_empty() { return; }
    println!();
    println!("  {}", "── Conversation ──".bold());
    for msg in messages {
        let role_label = match msg.role.as_str() {
            "user" => format!("  {} ", "[You]".green().bold()),
            "assistant" => format!("  {} ", "[Zap]".cyan().bold()),
            _ => format!("  {} ", format!("[{}]", msg.role).dimmed()),
        };
        let mut first = true;
        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => {
                    let prefix = format!("  {} ", "│".dimmed());
                    for line in text.lines() {
                        if first {
                            println!("{}{}", role_label, line.dimmed());
                            first = false;
                        } else if line.is_empty() {
                            println!("{}", prefix);
                        } else {
                            println!("{}{}", prefix, line.dimmed());
                        }
                    }
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    let input_str = serde_json::to_string(input)
                        .unwrap_or_else(|_| "[serialization error]".to_string());
                    let preview = truncate_preview(&input_str, 77);
                    println!("  {} {} {}", "▸".yellow(), name.yellow(), preview.dimmed());
                    first = false;
                }
                ContentBlock::ToolResult { content, .. } => {
                    let preview = truncate_preview(content, 117);
                    println!("  {} {}", "◂".blue(), preview.dimmed());
                    first = false;
                }
                ContentBlock::Image { media_type, .. } => {
                    println!("  {} [image: {}]", "🖼".dimmed(), media_type.dimmed());
                    first = false;
                }
                ContentBlock::Thinking { thinking, .. } => {
                    let preview = truncate_preview(thinking, 77);
                    println!("  {} {}", "💭".dimmed(), preview.dimmed());
                    first = false;
                }
                ContentBlock::Reasoning { content } => {
                    let preview = truncate_preview(content, 77);
                    println!("  {} {}", "🧠".dimmed(), preview.dimmed());
                    first = false;
                }
            }
        }
    }
    println!("  {}", "──────────────────".dimmed());
    println!();
}

impl Session {
    pub fn cmd_clear(&mut self) {
        self.messages.clear();
        println!("  {} History cleared.", "✓".green());
    }

    pub fn cmd_permissions(&mut self, arg: &str) {
        let new_mode = match arg.trim().to_lowercase().as_str() {
            "ask"  => PermissionMode::Ask,
            "auto" => PermissionMode::Auto,
            "deny" => PermissionMode::Deny,
            _ => {
                println!("  {} Usage: /permissions ask|auto|deny", "✗".red());
                return;
            }
        };
        self.permissions.mode = new_mode;
        println!("  {} Permission mode set to {}", "✓".green(), arg.trim().cyan().bold());
    }

    pub fn cmd_model(&mut self, name: &str, config: &Config) {
        self.model = name.to_string();
        let mut new_config = config.clone();
        new_config.model   = name.to_string();
        self.client = crate::llm_client::create_client(&new_config);
        println!("  {} Switched to {}", "✓".green(), name.cyan().bold());
    }

    /// Summarise conversation history in-place. Returns true on success.
    /// Tracks failures in `self.compact_failures`; resets to 0 on success.
    pub async fn cmd_compact(&mut self) -> bool {
        if self.messages.is_empty() {
            println!("  {} Nothing to compact.", "✗".red());
            return false;
        }
        let mut spinner = Self::make_spinner();
        let mut temp = self.messages.clone();
        temp.push(Message::user_text(
            "Please provide a concise summary of this conversation so far. \
             Include: the original task or goal, key decisions made, files created or \
             modified, errors encountered and how they were resolved, and the current \
             state. Preserve any explicit user instructions or preferences. \
             This summary will replace the full conversation history.",
        ));

        let result = self.client.send(
            "You are a helpful assistant. Summarize the conversation concisely, \
             preserving all task-critical context and user instructions.",
            &temp, &[], None, 0,
        ).await;
        spinner.finish_and_clear();

        match result {
            Ok(resp) => {
                let summary = resp.content.iter()
                    .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                    .collect::<Vec<_>>().join("\n");

                let turn_count = self.messages.len();
                self.messages.clear();
                self.messages.push(Message::user_text(format!(
                    "[Conversation compacted from {} messages]\n\n{}", turn_count, summary
                )));
                self.messages.push(Message {
                    role:    "assistant".to_string(),
                    content: vec![ContentBlock::Text {
                        text: "Understood. I have the context from our previous conversation.".to_string(),
                    }],
                });
                if let Some(u) = resp.usage {
                    self.session_usage.input_tokens  += u.input_tokens;
                    self.session_usage.output_tokens += u.output_tokens;
                }
                let _ = audit::record(&format!(
                    "compact: {} messages → 2 (summary) model={}", turn_count, self.config.model
                ));
                self.compact_failures = 0;
                println!("  {} Compacted {} messages into a summary.", "✓".green(), turn_count);
                true
            }
            Err(e) => {
                self.compact_failures += 1;
                println!("  {} Compact failed: {}", "✗".red(), e);
                if self.compact_failures >= 3 {
                    println!(
                        "  {} Compact failed 3 times — use {} to reset or {} to start fresh.",
                        "⚠".bright_yellow(), "/clear".cyan(), "/exit".cyan()
                    );
                }
                false
            }
        }
    }

    pub fn cmd_sessions(&mut self, _arg: &str) {
        let rows = match self.store.recent_sessions(20) {
            Ok(r) if r.is_empty() => { println!("  No sessions found."); return; }
            Ok(r) => r,
            Err(e) => { println!("  {} {}", "✗".red(), e); return; }
        };

        let cfg = crate::ui::inquire_render_config();
        let entries: Vec<String> = rows.iter().map(|(id, goal, model, ts)| {
            let date       = ts.get(..10).unwrap_or(ts.as_str());
            let goal_short = if goal.len() > 50 { format!("{}…", &goal[..49]) } else { goal.clone() };
            format!("#{:<4} {:<52} [{:<20}] {}", id, goal_short, model, date)
        }).collect();

        let result = Select::new("Resume session:", entries.clone())
            .with_render_config(cfg)
            .with_help_message("↑↓ navigate   type to filter   Enter load   Esc cancel")
            .with_page_size(12)
            .prompt_skippable();

        let chosen_idx = match result {
            Ok(Some(line)) => entries.iter().position(|e| e == &line),
            _ => None,
        };

        if let Some(idx) = chosen_idx {
            let (session_id, goal, model, _) = &rows[idx];
            let files_info = crate::project::session_log_files(*session_id)
                .map(|f| format!("  {} Files: {}", "◌".dimmed(), f.dimmed()))
                .unwrap_or_default();
            match self.store.load_messages(*session_id) {
                Ok(Some(json)) => match serde_json::from_str::<Vec<Message>>(&json) {
                    Ok(msgs) => {
                        self.messages   = msgs;
                        self.turn_count = self.messages.iter().filter(|m| m.role == "user").count();
                        self.session_id = *session_id;
                        self.model      = model.clone();
                        let mut cfg     = self.config.clone();
                        cfg.model       = model.clone();
                        self.client     = crate::llm_client::create_client(&cfg);
                        println!("  {} Loaded session #{} — {} messages, model {}",
                            "✓".green(), session_id, self.messages.len().to_string().cyan(), model.cyan());
                        println!("  {} {}", "◌".dimmed(), goal.dimmed());
                        if !files_info.is_empty() {
                            println!("{}", files_info);
                        }
                        print_conversation(&self.messages);
                    }
                    Err(e) => println!("  {} Could not parse messages: {}", "✗".red(), e),
                },
                Ok(None) => {
                    println!("  {} Session #{} has no saved messages.", "◌".yellow(), session_id);
                    println!("  {} {}", "◌".dimmed(), goal.dimmed());
                    if !files_info.is_empty() {
                        println!("{}", files_info);
                    }
                },
                Err(e)   => println!("  {} {}", "✗".red(), e),
            }
        }
    }
}
