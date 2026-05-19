/// Core agent session: struct, initialisation, tool loop, and slash dispatcher.
/// Slash-command implementations live in `commands` to keep this file focused.
pub mod commands;
use anyhow::Result;
use colored::Colorize;
use futures::future::join_all;
use std::sync::{Arc, Mutex};
use std::sync::atomic::Ordering;

use crate::{
    audit,
    config::{Config, Provider},
    context_manager,
    llm_client::{create_client, BeforeOutput, ContentBlock, LlmProvider, Message, Usage},
    permission_manager::PermissionManager,
    persistence,
    tools::{SpawnAgentTool, ToolRegistry},
    ui::{format_cost, tool_icon, ThinkingSpinner},
};

pub const MAX_TURNS: usize = 50;

/// Print a truncated inline preview of tool output so the user can see what happened
/// even if the LLM produces no follow-up text.
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

// ── Context window helpers ─────────────────────────────────────────────────────

/// Best-effort context window size for known model families.
pub fn model_context_limit(model: &str) -> usize {
    let m = model.to_lowercase();
    if m.contains("claude")                                    { 200_000 }
    else if m.contains("gemini-1.5") || m.contains("gemini-2") { 1_000_000 }
    else if m.contains("gemini")                               { 128_000 }
    else if m.contains("gpt-4o") || m.contains("gpt-4-turbo")
         || m.contains("o3") || m.contains("o4")               { 128_000 }
    else if m.contains("gpt-3.5")                              { 16_385 }
    else                                                        { 32_768 } // local default
}

/// Renders a 10-block ASCII bar: `[████████░░] 80%`
fn ctx_bar(pct: u8) -> String {
    let filled = (pct as usize).min(100) * 10 / 100;
    let bar: String = (0..10).map(|i| if i < filled { '█' } else { '░' }).collect();
    format!("[{}] {}%", bar, pct)
}

/// Heuristic: returns true when the message looks like a fresh topic rather than
/// a continuation of the current conversation.
fn is_topic_shift(input: &str, messages: &[Message]) -> bool {
    // Need ≥3 prior user turns to have a baseline.
    let user_texts: Vec<&str> = messages.iter()
        .filter(|m| m.role == "user")
        .flat_map(|m| m.content.iter())
        .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
        .collect();
    if user_texts.len() < 3 || input.len() < 40 { return false; }

    // Continuation signals in the first 6 words → followup.
    let lower = input.to_lowercase();
    let head: Vec<&str> = lower.split_whitespace().take(6).collect();
    let cont_words = ["it", "this", "that", "these", "those", "its", "above",
                      "also", "additionally", "furthermore", "now", "next"];
    if head.iter().any(|w| cont_words.contains(w)) { return false; }
    if lower.starts_with("and ") || lower.starts_with("but ") { return false; }

    let stop: std::collections::HashSet<&str> = [
        "the","a","an","and","or","but","in","on","at","to","for","of","with",
        "by","from","is","are","was","were","be","been","have","has","had","do",
        "does","did","will","would","could","should","may","might","can","this",
        "that","these","those","i","you","we","it","they","my","your","our",
        "please","help","make","add","create","want","need","like","just","how",
    ].iter().cloned().collect();

    let sig_words = |text: &str| -> std::collections::HashSet<String> {
        text.split_whitespace()
            .map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphabetic()).to_string())
            .filter(|w| w.len() > 4 && !stop.contains(w.as_str()))
            .collect()
    };

    let recent: std::collections::HashSet<String> = user_texts.iter()
        .rev().take(3)
        .flat_map(|t| sig_words(t))
        .collect();
    let incoming = sig_words(input);

    if incoming.is_empty() || recent.is_empty() { return false; }
    let overlap = incoming.intersection(&recent).count();
    (overlap as f64 / incoming.len() as f64) < 0.15
}

// ── Per-turn tool selection ───────────────────────────────────────────────────

/// Return the tool definitions to send with a single LLM call.
///
/// Anthropic: always send everything — prompt caching makes repeated tool
/// schemas essentially free from turn 2 onward (~10% of input token price).
///
/// OpenAI-compatible (local / LM Studio): smaller models benefit from a
/// tighter tool set — fewer choices means fewer hallucinated calls and fewer
/// wasted tokens.  We keep all coding tools always and gate `web_fetch` /
/// `web_search` behind a keyword check so they don't bloat every request.
fn select_tools_for_turn<'a>(
    all: &'a [serde_json::Value],
    user_input: &str,
    config: &crate::config::Config,
    messages: &[crate::llm_client::Message],
) -> std::borrow::Cow<'a, [serde_json::Value]> {
    use crate::config::Provider;

    // Anthropic has prompt caching — no filtering needed.
    if matches!(config.provider, Provider::Anthropic) {
        return std::borrow::Cow::Borrowed(all);
    }

    // Check current-turn keywords.
    let lower = user_input.to_lowercase();
    let wants_web_now = ["http://", "https://", "url", " web ", "website",
                         "fetch ", "curl ", "download", "browse", "docs",
                         "documentation", "web_fetch", "web_search"]
        .iter()
        .any(|kw| lower.contains(kw));

    // Keep web tools if already used earlier this session (sticky).
    let web_used = messages.iter().any(|m| {
        m.content.iter().any(|b| matches!(
            b,
            crate::llm_client::ContentBlock::ToolUse { name, .. }
            if name == "web_fetch" || name == "web_search"
        ))
    });

    if wants_web_now || web_used {
        return std::borrow::Cow::Borrowed(all);
    }

    // Drop web tools — everything else always included.
    let filtered: Vec<serde_json::Value> = all
        .iter()
        .filter(|def| {
            !matches!(
                def["name"].as_str().unwrap_or(""),
                "web_fetch" | "web_search"
            )
        })
        .cloned()
        .collect();
    std::borrow::Cow::Owned(filtered)
}

// ── Session ───────────────────────────────────────────────────────────────────

pub struct Session {
    pub client:        Box<dyn LlmProvider>,
    pub tools:         ToolRegistry,
    pub permissions:   PermissionManager,
    pub system:        String,
    pub tool_defs:     Vec<serde_json::Value>,
    pub messages:      Vec<Message>,
    pub model:         String,
    pub base_url:      Option<String>,
    pub session_usage: Usage,
    pub turn_count:    usize,
    pub tool_count:    usize,
    pub session_id:    i64,
    pub config:        Config,
    /// Images staged with /attach, sent with the next user turn then cleared.
    pub staged_images: Vec<(String, String)>,
    pub skills:        Vec<crate::skill_manager::Skill>,
    pub current_branch: String,
    pub code_index:    Arc<Mutex<crate::code_index::CodeIndex>>,
    pub store:         persistence::Store,
    pub hooks:         crate::hooks::HookRunner,
    /// Extended thinking token budget. 0 = disabled. Anthropic only.
    pub thinking_budget: u32,
}

impl Session {
    pub async fn new(config: &Config) -> Result<Self> {
        crate::http::init(config);
        let store = persistence::init()?;
        let session_id = store.save_session("(repl)", &config.model)?;

        let mut system = context_manager::build_system_prompt(config)?;
        let mut tools = ToolRegistry::new();

        // Lazy MCP: parse .mcp.json but don't spawn any processes yet.
        // The LLM will call mcp_connect("name") when it needs a server.
        let mcp_cfg = crate::mcp::load_config();
        let mcp_server_count = mcp_cfg.servers.len();
        let mcp_had_config = mcp_cfg.had_config;
        if mcp_server_count > 0 {
            tools.load_mcp_lazy(mcp_cfg);
        }

        if config.agent_depth > 0 {
            tools.register(std::sync::Arc::new(SpawnAgentTool::new(config.clone())));
        }
        // Inject MCP server manifest into system prompt so the LLM knows what
        // servers are available without loading any of their tools yet.
        if tools.has_pending_mcp() {
            let server_lines: String = tools
                .pending_mcp_servers()
                .iter()
                .map(|(name, desc)| format!(
                    "- {name}: {}",
                    desc.unwrap_or("MCP server")
                ))
                .collect::<Vec<_>>()
                .join("\n");
            system.push_str(
                "\n\n## MCP Servers (lazy-loaded)\n\
                 Use `mcp_connect` with the server name to load its tools on demand.\n"
            );
            system.push_str(&server_lines);
        }

        let tool_defs  = tools.tool_definitions();
        let tool_count = tool_defs.len();

        let skills      = crate::skill_manager::load_all_skills(&config.skill_paths);
        let always_on   = crate::skill_manager::always_on_skills(&skills);
        let stack_skills = crate::skill_manager::detect_stack_skills(&skills);

        // Bake always-on skills into the base system prompt once at startup.
        if !always_on.is_empty() {
            let block = crate::skill_manager::build_always_on_prompt(&always_on);
            system.push_str("\n\n");
            system.push_str(&block);
        }

        if !skills.is_empty() {
            let mut notes: Vec<String> = Vec::new();
            if !always_on.is_empty() {
                let names: Vec<_> = always_on.iter().map(|s| s.name.as_str()).collect();
                notes.push(format!("always-on: {}", names.join(", ")));
            }
            if !stack_skills.is_empty() {
                let names: Vec<_> = stack_skills.iter().map(|s| s.name.as_str()).collect();
                notes.push(format!("auto: {}", names.join(", ")));
            }
            let note = if notes.is_empty() { String::new() } else {
                format!("  {}", notes.join("  ·  ").dimmed())
            };
            if !config.is_subagent {
                println!(
                    "  {} {} skill(s) loaded{}",
                    "◎".truecolor(255, 200, 60),
                    skills.len().to_string().cyan(),
                    note,
                );
            }
        }

        let hooks = crate::hooks::HookRunner::load();
        if !hooks.is_empty() && !config.is_subagent {
            println!(
                "  {} {} hook(s) loaded",
                "◎".truecolor(255, 160, 80),
                hooks.total().to_string().cyan(),
            );
        }

        if mcp_server_count > 0 && !config.is_subagent {
            let names: Vec<String> = tools
                .pending_mcp_servers()
                .iter()
                .map(|(n, _)| (*n).to_string())
                .collect();
            println!(
                "  {} {} MCP server(s) ready on demand: {}",
                "◎".truecolor(255, 140, 60),
                mcp_server_count.to_string().cyan(),
                names.join(", ").dimmed(),
            );
        } else if mcp_had_config && !config.is_subagent {
            println!(
                "  {} {}",
                "○".truecolor(180, 120, 60),
                "MCP config found but no runnable stdio servers — all entries are disabled or use SSE/HTTP transport  (/mcp to edit)".truecolor(150, 120, 80),
            );
        }

        if !config.is_subagent {
            if let Some(summary) = crate::http::network_summary(config) {
                println!(
                    "  {} {}",
                    "◎".truecolor(180, 180, 100),
                    summary.dimmed(),
                );
            }
        }

        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        // Open the index DB but do NOT scan on startup — scanning can be slow on
        // large directories. Use /index to trigger a manual scan.
        let code_index = {
            let idx = crate::code_index::CodeIndex::open(&cwd)
                .unwrap_or_else(|_| {
                    crate::code_index::CodeIndex::open(std::path::Path::new("/tmp")).unwrap()
                });
            let arc = Arc::new(Mutex::new(idx));
            crate::code_index::set_global(arc.clone());
            arc
        };

        Ok(Self {
            client: create_client(config),
            tools,
            permissions: PermissionManager::new(config.permission_mode.clone()),
            system,
            tool_defs,
            messages: Vec::new(),
            model: config.model.clone(),
            base_url: config.base_url.clone(),
            session_usage: Usage::default(),
            turn_count: 0,
            tool_count,
            session_id,
            config: config.clone(),
            staged_images: Vec::new(),
            skills,
            current_branch: "main".to_string(),
            code_index,
            store,
            hooks,
            thinking_budget: 0,
        })
    }

    pub fn make_spinner() -> ThinkingSpinner { ThinkingSpinner::new() }

    pub fn estimated_context_tokens(&self) -> usize {
        let chars: usize = self.messages.iter().map(|m| {
            m.content.iter().map(|b| match b {
                ContentBlock::Text { text }              => text.len(),
                ContentBlock::ToolUse { input, .. }      => input.to_string().len(),
                ContentBlock::ToolResult { content, .. } => content.len(),
                ContentBlock::Image { data, .. }         => data.len() / 4,
                ContentBlock::Thinking { thinking, .. }  => thinking.len() / 4,
            }).sum::<usize>()
        }).sum();
        chars / 4
    }

    /// Context fill as 0–100 percentage.
    /// Uses --budget if set, otherwise falls back to the model's default context window.
    pub fn context_fill_pct(&self) -> u8 {
        let tokens = self.estimated_context_tokens();
        let limit  = self.config.budget
            .map(|b| b as usize)
            .unwrap_or_else(|| model_context_limit(&self.model));
        ((tokens * 100) / limit).min(100) as u8
    }

    // ── Core tool loop ────────────────────────────────────────────────────────

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

        // ── Topic-shift warning ───────────────────────────────────────────────
        if self.turn_count >= 3 && is_topic_shift(input, &self.messages) {
            println!(
                "  {} Looks like a new topic — consider {} to fork or {} for a fresh session.",
                "💡".bright_yellow(),
                "/branch".cyan(),
                "/exit".cyan(),
            );
        }

        // ── Context pressure handling ─────────────────────────────────────────
        let ctx_pct = self.context_fill_pct();
        let ctx_limit_k = self.config.budget
            .map(|b| b as usize)
            .unwrap_or_else(|| model_context_limit(&self.model)) / 1000;
        let ctx_used_k  = (self.estimated_context_tokens() / 1000).max(1);

        // --budget hard stop: refuse new turns when at 100%.
        if self.config.budget.is_some() && ctx_pct >= 100 {
            println!(
                "  {} Token budget exhausted (~{}k tokens). Start a new session or use /compact.",
                "✗".red().bold(), ctx_used_k
            );
            return Ok(());
        }
        if ctx_pct >= 95 {
            println!(
                "  {} Context {}% full (~{}k/{}k) — compacting automatically…",
                "⚡".red().bold(), ctx_pct, ctx_used_k, ctx_limit_k
            );
            self.cmd_compact().await;
        } else if ctx_pct >= 80 {
            println!(
                "  {} Context {}% full (~{}k/{}k tokens).",
                "⚠".bright_yellow(), ctx_pct, ctx_used_k, ctx_limit_k
            );
            let choice = inquire::Select::new(
                "Context is getting full — what would you like to do?",
                vec!["Continue anyway", "Compact (summarize history)", "Start new session (exit after this turn)"],
            )
            .with_render_config(crate::ui::inquire_render_config())
            .prompt_skippable()
            .unwrap_or(None);
            match choice {
                Some("Compact (summarize history)") => { self.cmd_compact().await; }
                Some("Start new session (exit after this turn)") => {
                    println!(
                        "  {} Answering your question, then type {} to start fresh.",
                        "ℹ".cyan(), "/exit".cyan()
                    );
                }
                _ => {}
            }
        } else if ctx_pct >= 70 {
            println!(
                "  {} Context {}% full (~{}k/{}k) — use {} to free space.",
                "⚠".bright_yellow().dimmed(), ctx_pct, ctx_used_k, ctx_limit_k,
                "/compact".cyan()
            );
        }

        let matched_skills: Vec<&crate::skill_manager::Skill> =
            crate::skill_manager::match_skills(input, &self.skills);
        let skill_tokens_this_turn: usize = matched_skills.iter().map(|s| s.tokens()).sum();

        let effective_system = if matched_skills.is_empty() {
            self.system.clone()
        } else {
            let skill_summary = crate::skill_manager::skills_summary(&matched_skills);
            println!(
                "  {} skills: {}",
                "↳".truecolor(255, 200, 60),
                skill_summary.dimmed()
            );
            let skill_block = crate::skill_manager::build_skill_prompt(&matched_skills);
            context_manager::build_system_prompt_with_skills(&self.config, &skill_block)
                .unwrap_or_else(|_| self.system.clone())
        };

        let msg_tokens_estimate = input.len() / 4;

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

            // In TUI mode use a no-op spinner — the TUI event loop animates via
            // LlmChunk events. In CLI mode use the normal indicatif spinner.
            let mut spinner = if crate::tui::channel::is_tui_mode() {
                crate::ui::ThinkingSpinner::noop()
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
                    // Signal the spinner thread to stop and wait for it to fully
                    // exit before clearing the bar. Without this, indicatif can
                    // redraw after finish_and_clear() and erase streaming text
                    // (especially visible on Windows).
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
            let result = self.client
                .send(&effective_system, &self.messages, &turn_tools, Some(before_output), self.thinking_budget)
                .await;
            spinner.finish_and_clear();
            let response = result?;

            // Empty response: two known causes.
            // (a) Zero input_tokens → context window exceeded (server sends 200 OK but empty SSE).
            // (b) Non-zero input_tokens → proxy or gateway dropped the response body.
            if response.content.is_empty() {
                let input_tokens = response.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0);
                if input_tokens == 0 {
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

                // Compute cumulative session cost and push to TUI header.
                let (cost_in, cost_out) = crate::ui::cost_per_million(&self.model);
                let total_usd = (self.session_usage.input_tokens  as f64 * cost_in
                               + self.session_usage.output_tokens as f64 * cost_out)
                               / 1_000_000.0;
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::CostUpdate {
                    total_usd,
                });
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::ContextUpdate {
                    pct: post_pct,
                    turn: self.turn_count,
                });
            }

            audit::record(&format!(
                "llm_response turn={} stop_reason={}", turn, response.stop_reason
            ))?;

            // Always record the assistant turn in history so subsequent turns
            // have full context (text-only responses were previously not saved).
            self.messages.push(Message {
                role:    "assistant".to_string(),
                content: response.content.clone(),
            });

            let tool_calls: Vec<&ContentBlock> = response.content.iter()
                .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                .collect();

            if tool_calls.is_empty() {
                // If the model signaled tool_use but we parsed no tool blocks,
                // the proxy likely used a non-standard response format.
                if response.stop_reason == "tool_use" {
                    crate::zap_warn!(
                        "Model signaled stop_reason=tool_use but no tool calls were parsed. \
                         Your proxy may use a unified/normalized schema that differs from \
                         the Anthropic wire format. Check ~/.zap/llm.log for the raw response."
                    );
                }
                break;
            }

            // Phase 1: permissions — quick-check each call, then ONE grouped prompt
            // for anything that needs user input (instead of per-call prompts).
            struct ApprovedCall {
                id:    String,
                name:  String,
                input: serde_json::Value,
                ctx:   String,
            }
            let mut approved:        Vec<ApprovedCall>            = Vec::new();
            let mut tool_results:    Vec<ContentBlock>            = Vec::new();
            // Calls that need a user prompt: (id, name, ctx, input)
            let mut needs_prompt:    Vec<(String, String, String, serde_json::Value)> = Vec::new();

            for block in &tool_calls {
                let ContentBlock::ToolUse { id, name, input } = block else { continue };
                tracing::info!(tool = %name, "tool use requested");
                audit::record(&format!("tool_request name={} id={}", name, id))?;

                let ctx = self.tools.get(name)
                    .map(|t| t.permission_context(input))
                    .unwrap_or_default();

                match self.permissions.quick_check(name) {
                    crate::permission_manager::QuickDecision::Allow => {
                        // Even in Auto mode, destructive shell commands require
                        // an explicit confirmation before executing.
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
                // Suspend TUI so inquire prompt can take over the terminal.
                crate::tui::channel::suspend_for_prompt();
                let batch: Vec<(String, String, String)> = needs_prompt.iter()
                    .map(|(id, name, ctx, _)| (id.clone(), name.clone(), ctx.clone()))
                    .collect();
                let decisions = self.permissions.prompt_batch(&batch)?;
                crate::tui::channel::resume_from_prompt();
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

            // ── Lazy MCP connect: intercept before parallel execution ────────
            // mcp_connect is a phantom tool (in tool_defs but not in self.tools).
            // Handle it directly so it can mutate self.tools and rebuild self.tool_defs.
            let (mcp_calls, approved): (Vec<_>, Vec<_>) = approved
                .into_iter()
                .partition(|c| c.name == "mcp_connect");

            for call in mcp_calls {
                let server = call.input["server"].as_str().unwrap_or("").to_string();
                println!(
                    "  {} {} {}…",
                    "╭─".truecolor(70, 65, 90),
                    "⬡".truecolor(255, 140, 60),
                    format!("mcp_connect  {}", server).truecolor(100, 210, 255).bold(),
                );
                let t0 = std::time::Instant::now();
                let (content, ok) = match self.tools.connect_mcp(&server).await {
                    Ok(msg) => {
                        // Rebuild tool_defs so the next LLM call sees the new tools.
                        self.tool_defs = self.tools.tool_definitions();
                        (msg, true)
                    }
                    Err(e) => (format!("Failed to connect MCP server '{}': {}", server, e), false),
                };
                let ms = t0.elapsed().as_millis();
                println!(
                    "  {} {}  {}",
                    "╰─".truecolor(70, 65, 90),
                    if ok { "✓".truecolor(80, 210, 120) } else { "✗".truecolor(220, 80, 80) },
                    format!("{}ms", ms).truecolor(90, 85, 110),
                );
                tool_results.push(ContentBlock::ToolResult {
                    tool_use_id: call.id,
                    content,
                });
            }

            // Snapshot (name, input) for PostToolUse hooks before consuming `approved`.
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
                    // Notify TUI of tool start
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
                                    let preview = output.lines().take(3).collect::<Vec<_>>().join("\n");
                                    // Notify TUI of tool done
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
                                    // Cap tool result size so large outputs don't blow the context.
                                    const MAX_TOOL_CHARS: usize = 20_000;
                                    let content = if output.len() > MAX_TOOL_CHARS {
                                        format!(
                                            "{}\n\n[... truncated — output was {} chars, showing first {}]",
                                            &output[..MAX_TOOL_CHARS],
                                            output.len(),
                                            MAX_TOOL_CHARS,
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
                                    // Notify TUI of tool done (failure)
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
                                    ContentBlock::ToolResult {
                                        tool_use_id: call.id,
                                        content:     err_str,
                                    }
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

            // Reindex any files that tools reported they wrote to.
            for block in &tool_calls {
                if let ContentBlock::ToolUse { name, input, .. } = block {
                    if let Some(tool) = self.tools.get(name) {
                        if let Some(path_str) = tool.affected_path(input) {
                            crate::code_index::global_reindex_file(std::path::Path::new(path_str));
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
                for result in &tool_results {
                    if let ContentBlock::ToolResult { content, .. } = result {
                        let hits = crate::secret_scanner::scan(content);
                        if !hits.is_empty() {
                            crate::tui::channel::suspend_for_prompt();
                            println!("  {} possible secret(s) detected before cloud send:", "⚠".yellow().bold());
                            for h in &hits { println!("    {}", h.to_string().yellow()); }
                            print!("  send anyway? [y/N] ");
                            let _ = std::io::Write::flush(&mut std::io::stdout());
                            let mut ans = String::new();
                            std::io::stdin().read_line(&mut ans).ok();
                            crate::tui::channel::resume_from_prompt();
                            if !ans.trim().eq_ignore_ascii_case("y") {
                                println!("  {} aborted by user — secrets not sent", "✗".red());
                                return Ok(());
                            }
                        }
                    }
                }
            }

            self.messages.push(Message::tool_results(tool_results));
        }

        // Persist conversation after every turn.
        if let Ok(json) = serde_json::to_string(&self.messages) {
            let _ = self.store.save_messages(self.session_id, &json);
        }

        Ok(())
    }

    // ── Slash dispatcher ──────────────────────────────────────────────────────

    /// Returns true if the session should end.
    pub async fn handle_slash(&mut self, line: &str, config: &Config) -> bool {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).copied().unwrap_or("").trim();

        match cmd {
            "/help"        => self.cmd_help(),
            "/config"      => self.cmd_config(),
            "/history"     => self.cmd_history(),
            "/clear"       => self.cmd_clear(),
            "/cost"        => self.cmd_cost(),
            "/models"      => self.cmd_models().await,
            "/sessions"    => self.cmd_sessions(arg),
            "/provider"    => self.cmd_provider(config),
            "/memory"      => self.cmd_memory(arg),
            "/audit"       => self.cmd_audit(arg),
            "/compact"     => self.cmd_compact().await,
            "/attach"      => self.cmd_attach(arg),
            "/paste"       => self.cmd_paste(),
            "/skill"       => self.cmd_skill(arg).await,
            "/run"         => {
                if arg.is_empty() {
                    let workflows = crate::workflow::discover_workflows();
                    if workflows.is_empty() {
                        println!("  No workflows found. Create .zap/workflows/<name>.yaml");
                    } else {
                        println!("  Available workflows:");
                        for (name, _) in &workflows { println!("    {} {}", "◌".dimmed(), name.cyan()); }
                        println!("  Run with: {}", "/run <name>".dimmed());
                    }
                } else if let Err(e) = self.cmd_run_workflow(arg).await {
                    println!("  {} workflow error: {}", "✗".red(), e);
                }
            }
            "/workflow"    => {
                if arg.starts_with("new ") || arg.starts_with("new\t") {
                    let name = arg[4..].trim();
                    if name.is_empty() {
                        println!("  usage: /workflow new <name>");
                    } else {
                        match crate::workflow::scaffold_workflow(name) {
                            Ok(p)  => println!("  {} created {}", "✓".green(), p.display().to_string().cyan()),
                            Err(e) => println!("  {} {}", "✗".red(), e),
                        }
                    }
                } else {
                    println!("  usage: /workflow new <name>   create a workflow scaffold");
                }
            }
            "/hooks"       => crate::hooks::print_hooks_list(&self.hooks),
            "/mcp"         => self.cmd_mcp(arg),
            "/tasks"       => self.cmd_tasks().await,
            "/think"       => self.cmd_think(arg),
            "/index"       => self.cmd_index(arg),
            "/branch"      => self.cmd_branch(arg).await,
            "/branches"    => self.cmd_branches(),
            "/switch"      => self.cmd_switch(arg).await,
            "/merge"       => self.cmd_merge(arg).await,
            "/undo"        => {
                let path = if arg.is_empty() { "list" } else { arg };
                if path == "list" {
                    let snaps = crate::snapshot::list_snapshots();
                    if snaps.is_empty() {
                        println!("  No undo history this session.");
                    } else {
                        println!("  Undo available:");
                        for s in snaps { println!("    {}", s.cyan()); }
                    }
                } else {
                    match crate::snapshot::restore_snapshot(path) {
                        Ok(content) => println!("  {} Reverted '{}' ({} bytes)", "✓".green(), path.cyan(), content.len()),
                        Err(e)      => println!("  {} {}", "✗".red(), e),
                    }
                }
            }
            "/init" => {
                if let Some(prompt) = self.cmd_init() {
                    if let Err(e) = self.handle_user_turn(&prompt).await {
                        println!("  {} agent error: {}", "✗".red(), e);
                    }
                }
            }
            "/permissions" => self.cmd_permissions(arg),
            "/model"       => {
                if arg.is_empty() { println!("  Usage: /model <model-id>"); }
                else              { self.cmd_model(arg, config); }
            }
            "/exit" | "/quit" => return true,
            other => println!("  {} Unknown command {}. Try {}.",
                "✗".red(), other.yellow(), "/help".cyan()),
        }
        false
    }
}
