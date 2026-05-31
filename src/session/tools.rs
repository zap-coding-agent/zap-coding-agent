use anyhow::Result;
use colored::Colorize;
use futures::future::join_all;

use crate::{
    audit,
    config::Provider,
    llm_client::{ContentBlock, Message},
    ui::tool_icon,
};

use super::Session;
use super::preview::smart_tool_preview;

fn print_tool_output(output: &str) {
    let trimmed = output.trim();
    if trimmed.is_empty() { return; }
    const MAX_LINES: usize = 20;
    let lines: Vec<&str> = trimmed.lines().collect();
    let shown = lines.len().min(MAX_LINES);
    for line in &lines[..shown] {
        println!("    {}", line.truecolor(160, 155, 185));
    }
    if lines.len() > MAX_LINES {
        println!(
            "    {}",
            format!("… {} more lines", lines.len() - MAX_LINES).truecolor(100, 95, 130)
        );
    }
}

impl Session {
    /// Execute one round of tool calls: permissions → parallel execution → secret scan.
    ///
    /// Returns `Ok(None)` if the turn should be aborted (secrets rejected), or
    /// `Ok(Some(msg))` with the tool-result message to push into history.
    pub(super) async fn execute_tool_round(
        &mut self,
        calls: Vec<(String, String, serde_json::Value)>,
    ) -> Result<Option<Message>> {
        #[derive(Clone)]
        struct ApprovedCall {
            id:    String,
            name:  String,
            input: serde_json::Value,
            ctx:   String,
        }

        let mut approved:     Vec<ApprovedCall>                                = Vec::new();
        let mut tool_results: Vec<ContentBlock>                                = Vec::new();
        let mut needs_prompt: Vec<(String, String, String, serde_json::Value)> = Vec::new();

        // Phase 1: permissions — quick-check each call, batch prompt for anything needing input.
        for (id, name, input) in &calls {
            audit::record(&format!("tool_request name={} id={}", name, id))?;

            let ctx = self.tools.get(name)
                .map(|t| t.permission_context(input))
                .unwrap_or_default();

            let mut perm_decision = self.permissions.quick_check(name);
            if matches!(perm_decision, crate::permission_manager::QuickDecision::Allow)
                && self.tools.is_mcp_tool(name)
                && matches!(self.permissions.mode, crate::config::PermissionMode::Ask)
                && !self.permissions.is_session_granted(name)
            {
                perm_decision = crate::permission_manager::QuickDecision::NeedsPrompt;
            }
            match perm_decision {
                crate::permission_manager::QuickDecision::Allow => {
                    let force_prompt = if name == "shell" {
                        if let Some(cmd) = input["command"].as_str() {
                            crate::tools::shell::destructive_pattern(cmd)
                                .map(|reason| format!("[DESTRUCTIVE: {}]\n         {}", reason, ctx))
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(destructive_ctx) = force_prompt {
                        needs_prompt.push((id.clone(), name.clone(), destructive_ctx, input.clone()));
                    } else {
                        match self.hooks.fire_pre_tool_use(name, input) {
                            crate::hooks::HookDecision::Block(reason) => {
                                audit::record(&format!("tool_blocked name={} reason={}", name, reason))?;
                                tool_results.push(ContentBlock::ToolResult {
                                    tool_use_id: id.clone(),
                                    content:     format!("Blocked by hook: {}", reason),
                                });
                            }
                            crate::hooks::HookDecision::Allow => {
                                approved.push(ApprovedCall {
                                    id: id.clone(), name: name.clone(),
                                    input: input.clone(), ctx,
                                });
                            }
                        }
                    }
                }
                crate::permission_manager::QuickDecision::Deny => {
                    audit::record(&format!("tool_denied name={} id={}", name, id))?;
                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content:     "Permission denied by policy.".to_string(),
                    });
                }
                crate::permission_manager::QuickDecision::NeedsPrompt => {
                    needs_prompt.push((id.clone(), name.clone(), ctx, input.clone()));
                }
            }
        }

        // Batch prompt — one grouped UI for all pending calls.
        if !needs_prompt.is_empty() {
            let in_tui = crate::tui::channel::is_tui_mode();
            if !in_tui { crate::tui::channel::suspend_for_prompt(); }
            let batch: Vec<(String, String, String)> = needs_prompt.iter()
                .map(|(id, name, ctx, _)| (id.clone(), name.clone(), ctx.clone()))
                .collect();
            let decisions = self.permissions.prompt_batch(&batch).await?;
            if !in_tui { crate::tui::channel::resume_from_prompt(); }
            for (i, (id, name, ctx, input)) in needs_prompt.into_iter().enumerate() {
                if decisions[i] {
                    match self.hooks.fire_pre_tool_use(&name, &input) {
                        crate::hooks::HookDecision::Block(reason) => {
                            audit::record(&format!("tool_blocked name={} reason={}", name, reason))?;
                            tool_results.push(ContentBlock::ToolResult {
                                tool_use_id: id,
                                content:     format!("Blocked by hook: {}", reason),
                            });
                        }
                        crate::hooks::HookDecision::Allow => {
                            approved.push(ApprovedCall { id, name, input, ctx });
                        }
                    }
                } else {
                    audit::record(&format!("tool_denied name={} id={}", name, id))?;
                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id,
                        content:     "Permission denied by user.".to_string(),
                    });
                }
            }
        }

        // Phase 1b: mcp_connect — mutates tool registry, must run before parallel phase.
        let mut connect_calls: Vec<ApprovedCall> = Vec::new();
        approved.retain(|c| {
            if c.name == "mcp_connect" { connect_calls.push(c.clone()); false } else { true }
        });
        for call in connect_calls {
            let server_name = call.input["server"]
                .as_str()
                .unwrap_or("")
                .to_string();

            crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::ToolStart {
                id:    call.id.clone(),
                name:  "mcp_connect".to_string(),
                label: server_name.clone(),
            });
            if !crate::tui::channel::is_tui_mode() {
                println!(
                    "  {} {}  {}",
                    "╭─".truecolor(70, 65, 90),
                    "⬡ mcp_connect".truecolor(100, 210, 255).bold(),
                    server_name.truecolor(130, 120, 155),
                );
            }

            let t0 = std::time::Instant::now();
            let result_text = if server_name.is_empty() {
                "Error: server_name is required.".to_string()
            } else {
                match self.tools.connect_mcp(&server_name).await {
                    Ok(msg) => {
                        self.tool_defs = self.tools.tool_definitions();
                        msg
                    }
                    Err(e) => format!("Failed to connect to '{}': {}", server_name, e),
                }
            };
            let ms = t0.elapsed().as_millis();
            let success = !result_text.starts_with("Failed") && !result_text.starts_with("Error");

            crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::ToolDone {
                id:         call.id.clone(),
                elapsed_ms: ms as u64,
                success,
                preview:    result_text.clone(),
            });
            if !crate::tui::channel::is_tui_mode() {
                if success {
                    println!("  {} {}  {}",
                        "╰─".truecolor(70, 65, 90),
                        "✓".truecolor(80, 210, 120),
                        format!("{}ms", ms).truecolor(90, 85, 110));
                } else {
                    println!("  {} {} {}",
                        "╰─".truecolor(70, 65, 90),
                        "✗".truecolor(220, 80, 80),
                        result_text.truecolor(220, 80, 80));
                }
            }

            tool_results.push(ContentBlock::ToolResult {
                tool_use_id: call.id,
                content:     result_text,
            });
        }

        // Snapshot meta before consuming `approved` for PostToolUse hooks.
        let approved_meta: Vec<(String, serde_json::Value)> = approved.iter()
            .map(|c| (c.name.clone(), c.input.clone()))
            .collect();

        // Phase 2: execute approved tools in parallel.
        let exec_futures = approved.into_iter().map(|call| {
            let tool = self.tools.get(&call.name);
            async move {
                let icon = tool_icon(&call.name);
                let cancel_hint = if call.name == "shell" {
                    format!("  {}", "Ctrl+C to cancel".truecolor(110, 105, 130))
                } else {
                    String::new()
                };
                let ctx_display = if call.ctx.chars().count() > 52 {
                    format!("{}…", call.ctx.chars().take(51).collect::<String>())
                } else {
                    call.ctx.clone()
                };
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::ToolStart {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    label: ctx_display.clone(),
                });
                if !crate::tui::channel::is_tui_mode() {
                    println!(
                        "  {} {} {}  {}{}",
                        "╭─".truecolor(70, 65, 90),
                        icon,
                        call.name.truecolor(100, 210, 255).bold(),
                        ctx_display.truecolor(130, 120, 155),
                        cancel_hint,
                    );
                }
                let t0 = std::time::Instant::now();
                match tool {
                    Some(t) => {
                        let _ = audit::record(&format!(
                            "tool_execute name={} input={}",
                            call.name,
                            serde_json::to_string(&call.input).unwrap_or_default()
                        ));
                        match t.execute(call.input).await {
                            Ok(output) => {
                                let _ = audit::record(&format!("tool_success name={}", call.name));
                                let ms = t0.elapsed().as_millis();
                                let preview = smart_tool_preview(&call.name, &output);
                                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::ToolDone {
                                    id: call.id.clone(),
                                    elapsed_ms: ms as u64,
                                    success: true,
                                    preview,
                                });
                                if !crate::tui::channel::is_tui_mode() {
                                    println!("  {} {}  {}",
                                        "╰─".truecolor(70, 65, 90),
                                        "✓".truecolor(80, 210, 120),
                                        format!("{}ms", ms).truecolor(90, 85, 110));
                                    if t.shows_inline_output() {
                                        print_tool_output(&output);
                                    }
                                }
                                const MAX_TOOL_BYTES: usize = 20_000;
                                let content = if output.len() > MAX_TOOL_BYTES {
                                    let mut cut = MAX_TOOL_BYTES;
                                    while cut > 0 && !output.is_char_boundary(cut) { cut -= 1; }
                                    format!(
                                        "{}\n\n[... truncated — output was {} bytes, showing first {}]",
                                        &output[..cut], output.len(), cut,
                                    )
                                } else {
                                    output
                                };
                                ContentBlock::ToolResult { tool_use_id: call.id, content }
                            }
                            Err(e) => {
                                let _ = audit::record(&format!("tool_error name={} err={}", call.name, e));
                                let ms = t0.elapsed().as_millis();
                                let err_str = format!("Error: {}", e);
                                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::ToolDone {
                                    id: call.id.clone(),
                                    elapsed_ms: ms as u64,
                                    success: false,
                                    preview: err_str.clone(),
                                });
                                if !crate::tui::channel::is_tui_mode() {
                                    println!("  {} {}  {}",
                                        "╰─".truecolor(70, 65, 90),
                                        "✗".truecolor(220, 80, 80),
                                        format!("{}ms", ms).truecolor(90, 85, 110));
                                    if t.shows_inline_output() {
                                        println!("    {}", err_str.truecolor(220, 100, 100));
                                    }
                                }
                                ContentBlock::ToolResult { tool_use_id: call.id, content: err_str }
                            }
                        }
                    }
                    None => {
                        let _ = audit::record(&format!("tool_unknown name={}", call.name));
                        crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::ToolDone {
                            id: call.id.clone(),
                            elapsed_ms: 0,
                            success: false,
                            preview: format!("Unknown tool: {}", call.name),
                        });
                        if !crate::tui::channel::is_tui_mode() {
                            println!("  {} {} unknown tool",
                                "╰─".truecolor(70, 65, 90), "✗".truecolor(220, 80, 80));
                        }
                        ContentBlock::ToolResult {
                            tool_use_id: call.id,
                            content:     format!("Unknown tool: {}", call.name),
                        }
                    }
                }
            }
        });
        let new_results = join_all(exec_futures).await;

        // Fire PostToolUse hooks (informational — cannot block).
        for ((name, input), result) in approved_meta.iter().zip(new_results.iter()) {
            if let ContentBlock::ToolResult { content, .. } = result {
                self.hooks.fire_post_tool_use(name, input, content);
            }
        }

        // Reindex any files that tools wrote to.
        for (_, name, input) in &calls {
            if let Some(tool) = self.tools.get(name) {
                if let Some(path_str) = tool.affected_path(input) {
                    crate::code_index::global_reindex_file(std::path::Path::new(path_str));
                    self.files_changed.push(path_str.to_string());
                }
            }
        }

        tool_results.extend(new_results);

        // Warn before sending potential secrets to cloud.
        if matches!(self.config.provider, Provider::Anthropic)
            || self.config.base_url.as_deref().map(|u| {
                !u.contains("192.168.") && !u.contains("localhost") && !u.contains("127.0.0.1")
            }).unwrap_or(false)
        {
            for result in &mut tool_results {
                if let ContentBlock::ToolResult { content, .. } = result {
                    let hits = crate::secret_scanner::scan(content);
                    if !hits.is_empty() {
                        let summary = crate::secret_scanner::redact(content, &hits);
                        if crate::tui::channel::is_tui_mode() {
                            crate::tui::channel::tui_send(
                                crate::tui::channel::TuiEvent::Warning(summary),
                            );
                        } else {
                            println!("\x1b[31;1m  ⚠ {summary} — redacted before sending to cloud model.\x1b[0m");
                        }
                    }
                }
            }
        }

        // Inject mid-turn btw messages the user typed via Ctrl+B.
        let btw_msgs = crate::tui::channel::drain_btw();
        let mut tool_msg = Message::tool_results(tool_results);
        if !btw_msgs.is_empty() {
            let note = btw_msgs
                .iter()
                .map(|m| format!("↳ User note (added mid-turn): {m}"))
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(ContentBlock::Text { text }) = tool_msg.content.last_mut() {
                text.push_str(&format!("\n\n{note}"));
            } else {
                tool_msg.content.push(ContentBlock::Text { text: note });
            }
        }

        Ok(Some(tool_msg))
    }
}
