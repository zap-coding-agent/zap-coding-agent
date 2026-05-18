/// Public entry points: run (single-shot), run_repl (interactive), run_subagent.
///
/// All session state, slash-command handlers, and UI primitives live in
/// `session` and `ui` respectively — this file is intentionally thin.
use anyhow::Result;
use colored::Colorize;
use rustyline::{Editor, EventHandler, KeyEvent, Modifiers};
use std::sync::{Arc, Mutex};

use crate::{
    audit,
    config::{Config, OutputFormat},
    llm_client::ContentBlock,
    session::Session,
    ui::{show_command_picker, SlashHandler, ZapHelper},
};

// ── Single-shot goal ──────────────────────────────────────────────────────────

pub async fn run(goal: &str, config: &Config) -> Result<()> {
    audit::record(&format!("session_start goal=\"{}\" model={}", goal, config.model))?;
    let mut session = Session::new(config).await?;
    session.handle_user_turn(goal).await?;
    audit::record("session_end")?;

    if config.output_format == OutputFormat::Json {
        let response_text = session.messages.iter().rev()
            .find(|m| m.role == "assistant")
            .and_then(|m| m.content.iter().find_map(|b| {
                if let ContentBlock::Text { text } = b { Some(text.clone()) } else { None }
            }))
            .unwrap_or_default();

        let out = serde_json::json!({
            "goal": goal,
            "model": config.model,
            "response": response_text,
            "usage": {
                "input_tokens":  session.session_usage.input_tokens,
                "output_tokens": session.session_usage.output_tokens,
            }
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    }

    Ok(())
}

// ── Interactive REPL ──────────────────────────────────────────────────────────

pub async fn run_repl(config: &Config) -> Result<()> {
    audit::record(&format!("repl_start model={}", config.model))?;

    println!("  {} Loading…", "◌".bright_yellow());
    let mut session = Session::new(config).await?;
    session.hooks.fire_session_start();

    // ── Mode picker ───────────────────────────────────────────────────────────
    let mode = crate::task_planner::pick_session_mode();
    if mode == crate::task_planner::SessionMode::Task {
        match crate::task_planner::run_task_planning(
            session.client.as_ref(),
            &session.model,
            &session.skills,
        )
        .await
        {
            Ok(Some(plan)) => {
                // Pre-load the goal as first user message so the agent has context.
                let intro = format!(
                    "I'm starting a task session. Goal: {}\n\n\
                     The tasks.md has been created at .zap/tasks/{}/tasks.md\n\
                     Please read it and confirm you understand the plan before we start.",
                    plan.goal, plan.folder_name
                );
                if let Err(e) = session.handle_user_turn(&intro).await {
                    println!("  {} {}", "✗".red(), e);
                }
            }
            Ok(None) => {
                // User aborted planning — continue as Vibe.
            }
            Err(e) => {
                println!("  {} Planning failed: {} — continuing in Vibe mode.", "⚠".yellow(), e);
            }
        }
    }

    let tool_w = session.tool_count;
    let depth_note = if session.config.agent_depth > 0 {
        format!("  ·  sub-agents ×{}", session.config.agent_depth)
    } else {
        String::new()
    };
    println!(
        "  {} {} tools{}",
        "◉".truecolor(100, 220, 100),
        tool_w.to_string().cyan().bold(),
        depth_note.dimmed(),
    );
    println!();

    let rl_config = rustyline::config::Builder::new()
        .completion_type(rustyline::config::CompletionType::List)
        .build();
    let mut rl = Editor::with_config(rl_config)?;
    rl.set_helper(Some(ZapHelper));

    let slash_triggered = Arc::new(Mutex::new(false));
    rl.bind_sequence(
        KeyEvent::new('/', Modifiers::NONE),
        EventHandler::Conditional(Box::new(SlashHandler {
            triggered: slash_triggered.clone(),
        })),
    );

    let history_path = dirs::home_dir()
        .map(|h| h.join(".zap_history"))
        .unwrap_or_else(|| ".zap_history".into());
    let _ = rl.load_history(&history_path);

    loop {
        let branch_tag = if session.current_branch != "main" {
            format!(":{}", session.current_branch)
        } else {
            String::new()
        };
        let ctx_pct = session.context_fill_pct();
        let ctx_tag = if ctx_pct >= 85 {
            format!("|{}%", ctx_pct).red().bold().to_string()
        } else if ctx_pct >= 70 {
            format!("|{}%", ctx_pct).bright_yellow().to_string()
        } else if ctx_pct > 0 {
            format!("|{}%", ctx_pct).truecolor(90, 80, 110).to_string()
        } else {
            String::new()
        };
        let prompt = format!(
            "\n  {} {} ",
            format!("[{}{}{}]", session.turn_count + 1, branch_tag, ctx_tag).truecolor(90, 80, 110),
            "❯".truecolor(255, 210, 50).bold(),
        );
        match rl.readline(&prompt) {
            Ok(line) => {
                let slash = std::mem::replace(&mut *slash_triggered.lock().unwrap(), false);
                if slash {
                    if let Some(cmd) = show_command_picker() {
                        if session.handle_slash(&cmd, config).await {
                            break;
                        }
                    }
                    continue;
                }

                let input = line.trim().to_string();
                if input.is_empty() {
                    continue;
                }
                rl.add_history_entry(&input).ok();

                if input.starts_with('/') {
                    if session.handle_slash(&input, config).await {
                        break;
                    }
                    continue;
                }

                if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
                    break;
                }

                let turn_result = tokio::select! {
                    r = session.handle_user_turn(&input) => r,
                    _ = tokio::signal::ctrl_c() => {
                        println!(
                            "\n  {} Cancelled — shell processes stopped. Type {} to quit.",
                            "⚡".bright_yellow(),
                            "/exit".cyan(),
                        );
                        Ok(())
                    }
                };
                if let Err(e) = turn_result {
                    println!("  {} {}", "Error:".red().bold(), e);
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                println!("  (^C — type /exit to quit)");
                continue;
            }
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(e) => {
                println!("  {} readline error: {}", "✗".red(), e);
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
    session.hooks.fire_session_end();
    println!("\n  {} Goodbye.", "⚡".bright_yellow());
    audit::record("repl_end")?;
    Ok(())
}

// ── Sub-agent (spawned by SpawnAgentTool) ─────────────────────────────────────

pub async fn run_subagent(goal: &str, config: &Config) -> Result<String> {
    let mut sub_config = config.clone();
    sub_config.output_format = OutputFormat::Json;
    sub_config.agent_depth = config.agent_depth.saturating_sub(1);

    let depth_label = format!("[depth {}]", 3u8.saturating_sub(sub_config.agent_depth));
    println!(
        "  {} {} {}",
        "◈".bright_cyan(),
        "sub-agent".cyan().bold(),
        depth_label.dimmed(),
    );

    audit::record(&format!(
        "subagent_start goal=\"{}\" depth={}",
        &goal[..goal.len().min(80)],
        sub_config.agent_depth
    ))?;

    let mut session = Session::new(&sub_config).await?;
    session.handle_user_turn(goal).await?;

    let turns = session.turn_count;
    let total_tools: usize = session.messages.iter()
        .flat_map(|m| &m.content)
        .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
        .count();

    let response = session.messages.iter().rev()
        .find(|m| m.role == "assistant")
        .and_then(|m| m.content.iter().find_map(|b| {
            if let ContentBlock::Text { text } = b { Some(text.clone()) } else { None }
        }))
        .unwrap_or_default();

    println!(
        "  {} sub-agent done  {} turn(s)  {} tool call(s)",
        "◈".bright_cyan(),
        turns.to_string().cyan(),
        total_tools.to_string().cyan(),
    );
    audit::record("subagent_end")?;

    Ok(response)
}
