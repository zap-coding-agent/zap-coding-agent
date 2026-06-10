use anyhow::Result;
use colored::Colorize;
use std::sync::atomic::Ordering;

use crate::{
    audit,
    context_manager,
    llm_client::{BeforeOutput, ContentBlock, Message},
    ui::{format_cost, ThinkingSpinner},
};

use super::{Session, MAX_TURNS};
use super::casual::{is_casual_message, is_topic_shift, needs_prior_context};
use super::history::{ctx_bar, model_context_limit, select_tools_for_turn, windowed_history};

impl Session {
    pub async fn handle_user_turn(&mut self, input: &str) -> Result<()> {
        // Fire UserPromptSubmit hooks — any hook that prints to stdout modifies the prompt.
        let modified;
        let input = if !self.hooks.user_prompt_submit.is_empty() {
            if let Some(new_prompt) = self.hooks.fire_user_prompt_submit(input) {
                modified = new_prompt;
                modified.as_str()
            } else {
                input
            }
        } else {
            input
        };

        // In CLI mode only — TUI intercepts topic shifts before the turn starts.
        if !crate::tui::channel::is_tui_mode()
            && self.turn_count >= 3
            && is_topic_shift(input, &self.messages)
        {
            println!(
                "  {} Looks like a new topic — consider {} to fork or {} for a fresh session.",
                "💡".bright_yellow(), "/branch".cyan(), "/exit".cyan(),
            );
        }

        let disable_compact = std::env::var("DISABLE_COMPACT").is_ok();
        let ctx_limit_k = std::env::var("ZAP_MAX_CONTEXT_TOKENS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .or_else(|| self.config.budget.map(|b| b as usize))
            .unwrap_or_else(|| model_context_limit(&self.model)) / 1000;

        // Project skill token cost before compaction check.
        // This prevents a situation where the context looks fine (e.g. 75%) but the
        // matched skills push it past 100%, causing an LLM context overflow error.
        let is_casual = is_casual_message(input) && !needs_prior_context(input, &self.messages);
        let projected_skill_tokens: usize = if is_casual {
            0
        } else {
            let ms = crate::skill_manager::match_skills_scoped(input, &self.skills, &self.domain_scope);
            let mut injected: Vec<&crate::skill_manager::Skill> = ms.into_iter().collect();
            for skill in &self.skills {
                if self.pinned_skills.contains(&skill.name)
                    && !injected.iter().any(|s| s.name == skill.name)
                {
                    injected.push(skill);
                }
            }
            crate::skill_manager::rank_and_truncate_skills(injected, self.config.skill_token_budget, &self.pinned_skills)
                .iter().map(|s| s.tokens()).sum()
        };

        let base_ctx = self.estimated_context_tokens();
        let projected_ctx = base_ctx + projected_skill_tokens;
        let ctx_pct = self.context_fill_pct_with(projected_ctx);
        let proj_ctx_k = (projected_ctx / 1000).max(1);

        if self.config.budget.is_some() && ctx_pct >= 100 {
            if crate::tui::channel::is_tui_mode() {
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::Notice(
                    format!("✗ Token budget exhausted (~{}k tokens). Use /new to start fresh or /compact to free space.", proj_ctx_k)
                ));
            } else {
                println!(
                    "  {} Token budget exhausted (~{}k tokens). Start a new session or use /compact.",
                    "✗".red().bold(), proj_ctx_k
                );
            }
            return Ok(());
        }
        if !disable_compact && ctx_pct >= 90 && self.compact_failures < 3 {
            if crate::tui::channel::is_tui_mode() {
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::Notice(
                    format!("⟳ Context {}% (~{}k/{}k) — compacting…", ctx_pct, proj_ctx_k, ctx_limit_k)
                ));
            } else {
                println!(
                    "  {} Context {}% (~{}k/{}k) — compacting…",
                    "⟳".truecolor(200, 150, 60), ctx_pct, proj_ctx_k, ctx_limit_k,
                );
            }
            self.cmd_compact().await;
        }

        // is_casual was computed above in the compaction block — reuse it.
        // maybe_summarize_dropped_turns runs before skill injection so the summary is available.
        // match_skills_scoped + rank_and_truncate were called above for projected token calc;
        // matched_skills below recomputes the effective list to inject this turn.

        // Summarize any turns that just slid off the context window (non-casual only).
        // Must run before the inner tool-loop so the summary is ready for injection.
        if !is_casual {
            self.maybe_summarize_dropped_turns().await;
        }

        let matched_skills: Vec<&crate::skill_manager::Skill> = if is_casual {
            Vec::new()
        } else {
            let mut ms = crate::skill_manager::match_skills_scoped(input, &self.skills, &self.domain_scope);
            for skill in &self.skills {
                if self.pinned_skills.contains(&skill.name)
                    && !ms.iter().any(|s| s.name == skill.name)
                {
                    ms.push(skill);
                }
            }
            // Apply token budget: rank + truncate if total exceeds config.skill_token_budget
            crate::skill_manager::rank_and_truncate_skills(ms, self.config.skill_token_budget, &self.pinned_skills)
        };
        let skill_tokens_this_turn: usize = matched_skills.iter().map(|s| s.tokens()).sum();

        {
            let preview: String = input.chars().take(60).collect();
            let names: Vec<String> = matched_skills.iter().map(|s| s.name.clone()).collect();
            let reason = if matched_skills.is_empty() {
                Some(if is_casual { "casual".to_string() } else { "no match".to_string() })
            } else {
                None
            };
            self.skill_trace.push((self.turn_count + 1, preview, names, reason));
        }

        let effective_system = if is_casual {
            context_manager::build_casual_system_prompt(&self.config)
        } else if matched_skills.is_empty() {
            self.system.clone()
        } else {
            let skill_summary = crate::skill_manager::skills_summary(&matched_skills);
            if crate::tui::channel::is_tui_mode() {
                crate::tui::channel::tui_send(
                    crate::tui::channel::TuiEvent::ActiveSkill(skill_summary.clone())
                );
            } else {
                println!(
                    "  {} skills: {}",
                    "↳".truecolor(255, 200, 60),
                    skill_summary.dimmed()
                );
            }
            let skill_block = crate::skill_manager::build_skill_prompt(&matched_skills);
            format!("{}\n\n{}", self.system, skill_block)
        };

        // ── Inject structured edit ledger (survives sliding-window eviction) ───
        let effective_system = if !is_casual && !self.edited_files.is_empty() {
            let mut sorted: Vec<_> = self.edited_files.iter().collect();
            sorted.sort_by(|(_, a), (_, b)| b.last_turn.cmp(&a.last_turn)
                .then_with(|| b.ops_count.cmp(&a.ops_count)));
            let ledger: Vec<String> = sorted.iter()
                .take(20)
                .map(|(p, e)| e.summary(p))
                .collect();
            let block = format!("── Edit Ledger ──\nFiles modified this session (persists across context windows):\n  {}\n",
                ledger.join("\n  "));
            format!("{}\n\n{}", effective_system, block)
        } else {
            effective_system
        };

        let msg_tokens_estimate = input.len() / 4;

        // Repair orphaned tool_use blocks from an interrupted previous turn.
        if let Some(last) = self.messages.last() {
            if last.role == "assistant" {
                let orphaned: Vec<String> = last.content.iter()
                    .filter_map(|b| {
                        if let ContentBlock::ToolUse { id, .. } = b { Some(id.clone()) } else { None }
                    })
                    .collect();
                if !orphaned.is_empty() {
                    let synthetic: Vec<ContentBlock> = orphaned.into_iter()
                        .map(|id| ContentBlock::ToolResult {
                            tool_use_id: id,
                            content: "Turn cancelled — result unavailable.".to_string(),
                        })
                        .collect();
                    self.messages.push(Message::tool_results(synthetic));
                }
            }
        }

        let user_msg = if self.staged_images.is_empty() {
            Message::user_text(input)
        } else {
            let mut blocks: Vec<ContentBlock> = self.staged_images.drain(..)
                .map(|(mime, data)| ContentBlock::Image { media_type: mime, data })
                .collect();
            blocks.push(ContentBlock::Text { text: input.to_string() });
            Message { role: "user".to_string(), content: blocks }
        };
        self.messages.push(user_msg);
        self.turn_count += 1;
        audit::record(&format!("user_turn: {}", input))?;

        if self.turn_count == 1 {
            let short = if input.len() > 80 { &input[..80] } else { input };
            let _ = self.store.update_session_goal(self.session_id, short);
        }

        for turn in 0..MAX_TURNS {
            tracing::info!(turn = turn, "calling LLM");

            let mut spinner = if crate::tui::channel::is_tui_mode() {
                ThinkingSpinner::noop()
            } else {
                Self::make_spinner()
            };
            let before_output: BeforeOutput = if crate::tui::channel::is_tui_mode() {
                Box::new(|| {})
            } else {
                let pb_clone      = spinner.pb_clone();
                let stop_clone    = spinner.stop_signal();
                let stopped_clone = spinner.stopped_signal();
                let model_label   = self.model.clone();
                Box::new(move || {
                    stop_clone.store(true, Ordering::Release);
                    let deadline = std::time::Instant::now()
                        + std::time::Duration::from_millis(200);
                    while !stopped_clone.load(Ordering::Acquire)
                        && std::time::Instant::now() < deadline
                    {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    pb_clone.finish_and_clear();
                    println!("  {} {}",
                        "╭─".truecolor(70, 65, 90),
                        model_label.truecolor(100, 95, 130));
                })
            };

            let turn_tools = select_tools_for_turn(
                &self.tool_defs, input, &self.config, &self.messages,
            );
            let effective_tools: &[serde_json::Value] = if is_casual { &[] } else { &turn_tools };
            let effective_msgs_owned: Vec<Message> = if is_casual {
                self.messages.last().cloned().into_iter().collect()
            } else {
                let windowed = windowed_history(&self.messages);
                if self.dropped_summary.is_empty() {
                    windowed
                } else {
                    // Prepend the LLM-generated summary of dropped turns so the model
                    // retains context from before the sliding window start.
                    let mut msgs = Vec::with_capacity(windowed.len() + 2);
                    msgs.push(Message::user_text(format!(
                        "[Context from earlier in this session — turns that slid off the \
                         context window]\n\n{}",
                        &self.dropped_summary
                    )));
                    msgs.push(Message {
                        role:    "assistant".to_string(),
                        content: vec![crate::llm_client::ContentBlock::Text {
                            text: "Understood — I have the earlier context.".to_string(),
                        }],
                    });
                    msgs.extend(windowed);
                    msgs
                }
            };
            let effective_messages: &[Message] = &effective_msgs_owned;
            let result = self.client
                .send(&effective_system, effective_messages, effective_tools, Some(before_output), self.thinking_budget)
                .await;
            spinner.finish_and_clear();

            let response = match result {
                Ok(r) => r,
                Err(e) => {
                    let msg = e.to_string().to_lowercase();
                    let is_overflow = msg.contains("too long")
                        || msg.contains("context_length_exceeded")
                        || msg.contains("maximum context length")
                        || (msg.contains("prompt") && msg.contains("long"));
                    let is_stream_drop = msg.contains("sse stream error")
                        || msg.contains("connection reset")
                        || msg.contains("connection closed")
                        || msg.contains("broken pipe")
                        || msg.contains("incomplete message");
                    if is_overflow && !disable_compact && self.compact_failures < 3 {
                        crate::zap_warn!("Prompt too long — compacting and retrying…");
                        if self.cmd_compact().await { continue; }
                    } else if is_stream_drop {
                        let notice = "⚠ Stream dropped by server — retrying in 3s…";
                        if crate::tui::channel::is_tui_mode() {
                            crate::tui::channel::tui_send(
                                crate::tui::channel::TuiEvent::LlmChunk(format!("\n{notice}"))
                            );
                        } else {
                            crate::zap_warn!("{}", notice);
                        }
                        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                        continue;
                    }
                    return Err(e);
                }
            };

            if response.content.is_empty() {
                let input_tokens = response.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0);
                if input_tokens == 0 {
                    if !disable_compact && self.compact_failures < 3 {
                        let ctx_k = self.estimated_context_tokens() / 1000;
                        crate::zap_warn!("Context ~{}k tokens exceeded limit — compacting and retrying…", ctx_k);
                        if self.cmd_compact().await { continue; }
                    }
                    let ctx_k = self.estimated_context_tokens() / 1000;
                    crate::zap_warn!(
                        "Model returned an empty response (context ~{}k tokens). \
                         Try /compact to free space, or increase the model's context window in LM Studio.",
                        ctx_k
                    );
                } else {
                    crate::zap_warn!(
                        "Model returned an empty response (stop_reason: {}, input_tokens: {}). \
                         Your proxy may have dropped the response body. \
                         Check ~/.zap/llm.log for the raw SSE stream.",
                        response.stop_reason, input_tokens
                    );
                }
                break;
            }

            if let Some(ref u) = response.usage {
                self.session_usage.input_tokens       += u.input_tokens;
                self.session_usage.output_tokens      += u.output_tokens;
                self.session_usage.cache_read_tokens  += u.cache_read_tokens;
                self.session_usage.cache_write_tokens += u.cache_write_tokens;

                let cost_str = format_cost(u, &self.model);
                let mut parts: Vec<String> = Vec::new();
                if skill_tokens_this_turn > 0 {
                    parts.push(format!("skills {}t", skill_tokens_this_turn));
                }
                if msg_tokens_estimate > 0 {
                    parts.push(format!("msg ~{}t", msg_tokens_estimate));
                }
                let post_pct = self.context_fill_pct();
                let bar = ctx_bar(post_pct);
                let bar_str = if post_pct >= 85 {
                    bar.red().bold().to_string()
                } else if post_pct >= 70 {
                    bar.bright_yellow().to_string()
                } else {
                    bar.truecolor(100, 95, 130).to_string()
                };

                if !crate::tui::channel::is_tui_mode() {
                    if parts.is_empty() {
                        println!("  {} {}", "╰─".truecolor(70, 65, 90), cost_str.truecolor(100, 95, 130));
                    } else {
                        println!("  {}", "╰─".truecolor(70, 65, 90));
                        println!("  {} {}  {}  {}",
                            "↳".truecolor(255, 200, 60),
                            parts.join("  ").truecolor(100, 95, 130),
                            "·".truecolor(70, 65, 90),
                            cost_str.truecolor(100, 95, 130));
                    }
                    if post_pct > 0 {
                        println!("  {} {}", "↳".truecolor(255, 200, 60), bar_str);
                    }
                }

                let (cost_in, cost_out) = crate::ui::cost_per_million(&self.model);
                let total_usd = (self.session_usage.input_tokens  as f64 * cost_in
                               + self.session_usage.output_tokens as f64 * cost_out)
                               / 1_000_000.0;
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::CostUpdate {
                    total_usd,
                    input:      self.session_usage.input_tokens,
                    output:     self.session_usage.output_tokens,
                    cache_read: self.session_usage.cache_read_tokens,
                });
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::ContextUpdate {
                    pct: post_pct,
                    turn: self.turn_count,
                });
            }

            audit::record(&format!(
                "llm_response turn={} stop_reason={}", turn, response.stop_reason
            ))?;

            self.messages.push(Message {
                role:    "assistant".to_string(),
                content: response.content.clone(),
            });

            let tool_calls: Vec<&ContentBlock> = response.content.iter()
                .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                .collect();

            if tool_calls.is_empty() {
                if response.stop_reason == "tool_use" {
                    crate::zap_warn!(
                        "Model signaled stop_reason=tool_use but no tool calls were parsed. \
                         Your proxy may use a unified/normalized schema that differs from \
                         the Anthropic wire format. Check ~/.zap/llm.log for the raw response."
                    );
                }
                break;
            }

            // Extract owned call data to pass across the async boundary.
            let calls: Vec<(String, String, serde_json::Value)> = tool_calls.iter()
                .filter_map(|b| {
                    if let ContentBlock::ToolUse { id, name, input } = b {
                        Some((id.clone(), name.clone(), input.clone()))
                    } else { None }
                })
                .collect();

            match self.execute_tool_round(calls).await? {
                None => return Ok(()),   // secrets abort
                Some(tool_msg) => self.messages.push(tool_msg),
            }

            // If memory_set / memory_delete ran this round, patch self.system so
            // the next LLM call in this session sees the updated facts.
            if crate::tools::take_dirty_flag() {
                self.patch_memory_in_system();
            }
        }

        // Drain btw messages not picked up mid-turn.
        let leftover_btw = crate::tui::channel::drain_btw();
        if !leftover_btw.is_empty() {
            crate::tui::channel::tui_send(
                crate::tui::channel::TuiEvent::BtwCarryover(leftover_btw)
            );
        }

        if let Ok(json) = serde_json::to_string(&self.messages) {
            let _ = self.store.save_messages(self.session_id, &json);
        }

        crate::remote_channel::send_done();

        Ok(())
    }
}
