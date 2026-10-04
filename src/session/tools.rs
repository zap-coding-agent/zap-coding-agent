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

/// Tools that mutate filesystem or process state must run strictly serial within
/// a single turn — the model may emit a write followed by a dependent read/build
/// in the same response, and parallel execution would race the writer.
/// Read-only tools stay parallel: that's where the frontier multi-read speedup lives.
fn is_mutating_tool(name: &str) -> bool {
    matches!(name,
        "shell" | "write_file" | "edit_file" | "str_replace" |
        "undo_edit" | "create_file" | "apply_patch"
    )
}

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
                    let destructive_reason = if name == "shell" {
                        input["command"].as_str().and_then(crate::tools::shell::destructive_pattern)
                    } else {
                        None
                    };
                    if let Some(reason) = destructive_reason {
                        if self.config.is_subagent {
                            // No controlling terminal to prompt (model-invoked spawn_agent
                            // or a /bg background agent) — auto-deny instead of queuing an
                            // interactive prompt that can never be answered.
                            audit::record(&format!("tool_denied name={} id={} reason=destructive_unattended", name, id))?;
                            tool_results.push(ContentBlock::ToolResult {
                                tool_use_id: id.clone(),
                                content: format!(
                                    "blocked: {reason} — destructive commands require interactive \
                                     approval, not available in an unattended sub-agent."
                                ),
                            });
                        } else {
                            let destructive_ctx = format!("[DESTRUCTIVE: {}]\n         {}", reason, ctx);
                            needs_prompt.push((id.clone(), name.clone(), destructive_ctx, input.clone()));
                        }
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
                input: call.input.clone(),
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
                        self.tool_defs = self.tools.tool_definitions_filtered(&self.config.disabled_tools);
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

        // Phase 2: execute approved tools.
        // - Read-only tools (read_file, list_directory, code_map, …) run in parallel
        //   — preserves the frontier multi-read speedup (Claude often emits 3+ reads/turn).
        // - Mutating tools (shell, write_file, edit_file, …) run strictly serial,
        //   in submission order — eliminates the file-dependency race that bit Devstral
        //   when it tried to run `npm install` while `npx create-vite` was still scaffolding.
        // Frontier models almost always emit ≤1 mutating call per turn, so for them
        // this collapses to "run that one call" — no parallelism to lose.
        let tools_ref = &self.tools;
        let exec_one = |call: ApprovedCall| async move {
            let tool = tools_ref.get(&call.name);
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
                input: call.input.clone(),
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
        };

        // Tag each call with its submission index, then partition by mutating-ness.
        let mut parallel_calls: Vec<(usize, ApprovedCall)> = Vec::new();
        let mut serial_calls: Vec<(usize, ApprovedCall)> = Vec::new();
        for (idx, call) in approved.into_iter().enumerate() {
            if is_mutating_tool(&call.name) {
                serial_calls.push((idx, call));
            } else {
                parallel_calls.push((idx, call));
            }
        }

        // Phase 2a: read-only tools in parallel.
        let parallel_results: Vec<(usize, ContentBlock)> = join_all(
            parallel_calls.into_iter().map(|(idx, call)| {
                let fut = exec_one(call);
                async move { (idx, fut.await) }
            })
        ).await;

        // Phase 2b: mutating tools strictly serial, in submission order.
        let mut serial_results: Vec<(usize, ContentBlock)> = Vec::new();
        for (idx, call) in serial_calls {
            serial_results.push((idx, exec_one(call).await));
        }

        // Merge back in submission order so tool_results match the model's call order.
        let mut indexed: Vec<(usize, ContentBlock)> =
            parallel_results.into_iter().chain(serial_results).collect();
        indexed.sort_by_key(|(i, _)| *i);
        let mut new_results: Vec<ContentBlock> =
            indexed.into_iter().map(|(_, r)| r).collect();

        // Fire PostToolUse hooks (informational — cannot block).
        for ((name, input), result) in approved_meta.iter().zip(new_results.iter()) {
            if let ContentBlock::ToolResult { content, .. } = result {
                self.hooks.fire_post_tool_use(name, input, content);
            }
        }

        // Verify-aware watchdog. Two stuck signals, both per verification
        // command: (a) the same command failing N times — catches a model
        // trying DIFFERENT broken fixes; (b) an outstanding failure not re-run
        // for R rounds — catches a model wandering in exit-0 probe commands
        // instead of re-verifying. A command passing clears its entry, so
        // healthy work (TDD red→green, diagnostics, long builds with periodic
        // re-verifies) never trips it.
        let breaker_n = super::watchdog::breaker_n();
        let breaker_r = super::watchdog::breaker_rounds();
        for ((name, input), result) in approved_meta.iter().zip(new_results.iter_mut()) {
            let ContentBlock::ToolResult { content, .. } = result else { continue };
            if name != "shell" { continue }
            let Some(cmd) = input["command"].as_str() else { continue };
            let key = super::watchdog::normalize_cmd(cmd);
            if super::watchdog::is_failed_verify(name, content) {
                let entry = self.verify_fails.entry(key).or_insert((0, self.agentic_round));
                entry.0 += 1;
                entry.1 = self.agentic_round;
            } else {
                self.verify_fails.remove(&key);
            }
        }

        if !self.verify_escalated && breaker_n > 0 {
            // Assess every outstanding failure; act on the worst verdict.
            let mut action: Option<(super::watchdog::WatchdogVerdict, String, u32, usize)> = None;
            for (cmd, (fails, last_round)) in &self.verify_fails {
                let stale_rounds = self.agentic_round.saturating_sub(*last_round);
                let verdict = super::watchdog::assess_outstanding(
                    *fails, stale_rounds, breaker_n, breaker_r, self.watchdog_nudged,
                );
                let worse = matches!(verdict, super::watchdog::WatchdogVerdict::Escalate)
                    || (action.is_none() && verdict != super::watchdog::WatchdogVerdict::Quiet);
                if worse {
                    let is_escalate = matches!(verdict, super::watchdog::WatchdogVerdict::Escalate);
                    action = Some((verdict, cmd.clone(), *fails, stale_rounds));
                    if is_escalate { break; }
                }
            }
            if let Some((verdict, cmd, fails, stale_rounds)) = action {
                let (text, note) = match verdict {
                    super::watchdog::WatchdogVerdict::Nudge => {
                        self.watchdog_nudged = true;
                        (super::watchdog::nudge_text(fails),
                         format!("⚠ watchdog: `{cmd}` failed {fails}× — forcing a rethink before more edits"))
                    }
                    super::watchdog::WatchdogVerdict::StaleNudge => {
                        self.watchdog_nudged = true;
                        (super::watchdog::stale_nudge_text(&cmd, stale_rounds),
                         format!("⚠ watchdog: `{cmd}` still failing, not re-verified for {stale_rounds} rounds"))
                    }
                    super::watchdog::WatchdogVerdict::Escalate => {
                        self.verify_escalated = true;
                        (super::watchdog::escalate_text(fails.max(1)),
                         format!("⚠ watchdog: `{cmd}` never went green ({fails} fails, {stale_rounds} rounds stale) — stopping, requesting escalation summary"))
                    }
                    super::watchdog::WatchdogVerdict::Quiet => unreachable!(),
                };
                if let Some(ContentBlock::ToolResult { content, .. }) = new_results.last_mut() {
                    content.push_str(&text);
                }
                if crate::tui::channel::is_tui_mode() {
                    crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::Warning(note));
                } else {
                    crate::zap_warn!("{}", note);
                }
            }
        }

        // Reindex any files that tools wrote to.
        for (_, name, input) in &calls {
            if let Some(tool) = self.tools.get(name) {
                if let Some(path_str) = tool.affected_path(input) {
                    crate::code_index::global_reindex_file(std::path::Path::new(path_str));
                    self.files_changed.push(path_str.to_string());
                    let entry = self.edited_files.entry(path_str.to_string());
                    let cur = self.turn_count;
                    match entry {
                        std::collections::hash_map::Entry::Occupied(mut o) => {
                            o.get_mut().last_turn = cur;
                            o.get_mut().ops_count += 1;
                        }
                        std::collections::hash_map::Entry::Vacant(v) => {
                            v.insert(crate::session::EditedFile { first_turn: cur, last_turn: cur, ops_count: 1 });
                        }
                    }

                    // Notify LSP server that file was saved via did_save notification.
                    if let Some(ref lsp_arc) = crate::lsp::global_lsp() {
                        let abs_path_str = std::fs::canonicalize(path_str)
                            .unwrap_or_else(|_| std::path::PathBuf::from(path_str))
                            .to_string_lossy()
                            .to_string();
                        let lang = crate::lsp::language_for_path(&abs_path_str);
                        if lang != "unknown" {
                            if let Ok(mgr) = lsp_arc.try_lock() {
                                mgr.notify_save(lang, &abs_path_str);
                            }
                        }
                    }
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
