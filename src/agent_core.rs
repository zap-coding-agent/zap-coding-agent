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
    config::{Config, OutputFormat, PermissionMode},
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

// ── TUI mode ─────────────────────────────────────────────────────────────────

pub async fn run_tui(config: &Config) -> Result<()> {
    audit::record(&format!("tui_start model={}", config.model))?;
    crate::tui::run_tui(config).await?;
    audit::record("tui_end")?;
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
                // Poisoned mutex: treat as "not triggered" rather than crashing the REPL.
                let slash = match slash_triggered.lock() {
                    Ok(mut g) => std::mem::replace(&mut *g, false),
                    Err(_)    => false,
                };
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
    session.save_context_with_summary().await;
    session.hooks.fire_session_end();
    println!("\n  {} Goodbye.", "⚡".bright_yellow());
    let _ = audit::record("repl_end");
    std::process::exit(0);
}

// ── SDK / headless mode ───────────────────────────────────────────────────────
//
// Reads newline-delimited JSON from stdin, runs each "user" turn through the
// session, and writes the assistant's response as JSON to stdout.
//
// stdin:  {"type":"user","text":"..."} | {"type":"quit"}
// stdout: {"type":"assistant","text":"...","turn":N,"ctx_pct":N}
//         {"type":"error","message":"..."}
//
// All non-JSON terminal output (tool call boxes, spinners, etc.) goes to stderr,
// keeping stdout clean for machine consumption.
pub async fn run_sdk(config: &Config) -> Result<()> {
    // Redirect coloured terminal noise to stderr in SDK mode.
    // The colored crate respects NO_COLOR; we set it so callers get clean stdout.
    std::env::set_var("NO_COLOR", "1");

    audit::record(&format!("sdk_start model={}", config.model))?;
    let mut session = Session::new(config).await?;

    use tokio::io::AsyncBufReadExt;
    let stdin  = tokio::io::stdin();
    let reader = tokio::io::BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() { continue; }

        let msg: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v)  => v,
            Err(e) => {
                let err = serde_json::json!({"type":"error","message": format!("bad JSON: {}", e)});
                println!("{}", err);
                continue;
            }
        };

        match msg["type"].as_str() {
            Some("user") => {
                let text = match msg["text"].as_str() {
                    Some(t) => t.to_string(),
                    None => {
                        let err = serde_json::json!({"type":"error","message":"missing 'text' field"});
                        println!("{}", err);
                        continue;
                    }
                };

                if let Err(e) = session.handle_user_turn(&text).await {
                    let err = serde_json::json!({"type":"error","message": e.to_string()});
                    println!("{}", err);
                    continue;
                }

                // Extract the last assistant text from the session.
                let response_text = session.messages.iter().rev()
                    .find(|m| m.role == "assistant")
                    .and_then(|m| m.content.iter().find_map(|b| {
                        if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
                    }))
                    .unwrap_or("")
                    .to_string();

                let out = serde_json::json!({
                    "type": "assistant",
                    "text": response_text,
                    "turn": session.turn_count,
                    "ctx_pct": session.context_fill_pct(),
                    "usage": {
                        "input_tokens":  session.session_usage.input_tokens,
                        "output_tokens": session.session_usage.output_tokens,
                    }
                });
                println!("{}", out);
            }
            Some("quit") | Some("exit") => break,
            other => {
                let err = serde_json::json!({"type":"error","message": format!("unknown type: {:?}", other)});
                println!("{}", err);
            }
        }
    }

    session.hooks.fire_session_end();
    audit::record("sdk_end")?;
    Ok(())
}

// ── Sub-agent (spawned by SpawnAgentTool) ─────────────────────────────────────

pub async fn run_subagent(goal: &str, config: &Config) -> Result<String> {
    let mut sub_config = config.clone();
    sub_config.output_format  = OutputFormat::Json;
    sub_config.agent_depth    = config.agent_depth.saturating_sub(1);
    sub_config.is_subagent    = true; // suppress startup banners
    sub_config.spawn_depth    = config.spawn_depth.saturating_add(1);
    // Sub-agents must run in Auto mode: they have no controlling terminal, so
    // prompting stdin would deadlock (parent session is blocking on this call).
    sub_config.permission_mode = PermissionMode::Auto;

    let depth_level = sub_config.spawn_depth;
    let short_goal: String = goal.chars().take(60).collect();
    println!(
        "  {} {} {}  {}",
        "◈".bright_cyan(),
        "sub-agent".cyan().bold(),
        format!("[L{}]", depth_level).dimmed(),
        short_goal.truecolor(120, 115, 140),
    );

    let audit_goal: String = goal.chars().take(80).collect();
    audit::record(&format!(
        "subagent_start goal=\"{}\" depth={}",
        audit_goal, depth_level
    ))?;

    let mut session = Session::new(&sub_config).await?;
    session.handle_user_turn(goal).await?;

    let turns = session.turn_count;
    let total_tools: usize = session.messages.iter()
        .flat_map(|m| &m.content)
        .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
        .count();

    // Collect files that were written/edited via the affected_path() trait method,
    // which is the canonical source of truth rather than hardcoded tool names.
    let mut files_changed: Vec<String> = session.messages.iter()
        .flat_map(|m| &m.content)
        .filter_map(|b| {
            if let ContentBlock::ToolUse { name, input, .. } = b {
                session.tools.get(name)?.affected_path(input).map(str::to_string)
            } else {
                None
            }
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    files_changed.sort_unstable();

    let summary = session.messages.iter().rev()
        .find(|m| m.role == "assistant")
        .and_then(|m| m.content.iter().find_map(|b| {
            if let ContentBlock::Text { text } = b { Some(text.clone()) } else { None }
        }))
        .unwrap_or_default();

    let result = serde_json::json!({
        "summary": summary,
        "turns": turns,
        "tool_calls": total_tools,
        "files_changed": files_changed,
        "input_tokens": session.session_usage.input_tokens,
        "output_tokens": session.session_usage.output_tokens,
    });

    println!(
        "  {} sub-agent [L{}] done  {} turn(s)  {} tool(s){}",
        "◈".bright_cyan(),
        depth_level,
        turns.to_string().cyan(),
        total_tools.to_string().cyan(),
        if files_changed.is_empty() {
            String::new()
        } else {
            format!("  changed: {}", files_changed.join(", ").truecolor(130, 125, 150))
        },
    );
    audit::record(&format!("subagent_end depth={} turns={} tools={}", depth_level, turns, total_tools))?;

    Ok(result.to_string())
}
