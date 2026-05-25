/// Core agent session: struct, initialisation, tool loop, and slash dispatcher.
/// Slash-command implementations live in `commands` to keep this file focused.
pub mod commands;
use anyhow::Result;
use colored::Colorize;
use futures::future::join_all;
use inquire::Confirm;
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
    else if m.contains("deepseek")                             { 64_000 }
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
    /// Names of Domain skills active this session. Empty = no restriction (all Domain candidates).
    pub domain_scope:  std::collections::HashSet<String>,
    /// Skills the user has explicitly pinned via `/skill use <name>` — injected every turn.
    pub pinned_skills: std::collections::HashSet<String>,
    pub current_branch: String,
    pub code_index:    Arc<Mutex<crate::code_index::CodeIndex>>,
    pub store:         persistence::Store,
    pub hooks:         crate::hooks::HookRunner,
    /// Extended thinking token budget. 0 = disabled. Anthropic only.
    pub thinking_budget: u32,
    /// Number of consecutive compact failures; gates auto-compact circuit breaker.
    pub compact_failures: u8,
    /// Paths written/edited this session — used to populate context.md at exit.
    pub files_changed: Vec<String>,
    /// Info lines shown at TUI startup (context banner, init nudge). Drained by run_tui().
    pub startup_notices: Vec<String>,
    /// Per-turn skill trace: (turn_number, input_preview, skill_names, reason_if_none).
    /// "reason_if_none" is Some("casual") or Some("no match") when skills=[].
    pub skill_trace: Vec<(usize, String, Vec<String>, Option<String>)>,
}

impl Session {
    pub async fn new(config: &Config) -> Result<Self> {
        crate::http::init(config);
        let store = persistence::init()?;
        let session_id = store.save_session("(repl)", &config.model)?;

        let mut system = context_manager::build_system_prompt(config)?;
        let mut tools = ToolRegistry::new();

        // MCP: load config into pending_mcp — servers connect on first use via mcp_connect tool.
        let mcp_cfg = crate::mcp::load_config();
        let mcp_had_config = mcp_cfg.had_config;
        let mcp_server_count = mcp_cfg.servers.len();
        if mcp_server_count > 0 {
            tools.load_mcp_lazy(mcp_cfg);
        }

        if config.agent_depth > 0 {
            tools.register(std::sync::Arc::new(SpawnAgentTool::new(config.clone())));
        }

        let tool_defs  = tools.tool_definitions();
        let tool_count = tool_defs.len();

        // First run: write bundled skills to ~/.zap/skills/ so users can view/edit them.
        // Skips any file that already exists — user edits are never overwritten.
        let _bootstrapped = crate::skill_manager::bootstrap_bundled_skills();

        let skills    = crate::skill_manager::load_all_skills(&config.skill_paths);
        let always_on = crate::skill_manager::always_on_skills(&skills);

        // Bake Core skills into the base system prompt once at startup.
        if !always_on.is_empty() {
            let block = crate::skill_manager::build_always_on_prompt(&always_on);
            system.push_str("\n\n");
            system.push_str(&block);
        }

        // ── C2: Load project.json — use persisted languages, skip domain prompt ─
        let project_meta = crate::project::load_project_meta();

        // Build domain scope: project.json takes priority, then manifest detection, then prompt.
        let detected = crate::skill_manager::detect_domain_scope(&skills);
        let domain_scope: std::collections::HashSet<String> = if let Some(ref meta) = project_meta {
            if !meta.language.is_empty() {
                // Languages already known — no prompt needed.
                meta.language.iter().cloned().collect()
            } else if !detected.is_empty() {
                detected.iter().cloned().collect()
            } else {
                std::collections::HashSet::new()
            }
        } else if !detected.is_empty() {
            detected.iter().cloned().collect()
        } else if !config.is_subagent && !config.skip_domain_prompt
                  && unsafe { libc::isatty(0 as libc::c_int) } != 0 {
            // Nothing auto-detected — ask the user once (TTY only).
            crate::skill_manager::prompt_domain_scope(&skills)
                .map(|v| v.into_iter().collect())
                .unwrap_or_default()
        } else {
            std::collections::HashSet::new()
        };

        // ── C3: Index nudge (shown once when project isn't indexed yet) ────────
        let index_nudge: Option<String> = if config.is_subagent {
            None
        } else {
            match &project_meta {
                Some(meta) if !meta.indexed =>
                    Some("Project not indexed — run /index for fast symbol lookup, or /init to re-run full setup.".into()),
                None =>
                    Some("Run /init to set up this project — language detection, tree-sitter indexing, and project context.".into()),
                _ => None,
            }
        };

        if !skills.is_empty() && !config.is_subagent && !config.tui_mode {
            let core_names: Vec<_> = always_on.iter().map(|s| s.name.as_str()).collect();
            let mut notes: Vec<String> = Vec::new();
            if !core_names.is_empty() {
                notes.push(format!("core: {}", core_names.join(", ")));
            }
            if !domain_scope.is_empty() {
                let mut names: Vec<&str> = domain_scope.iter().map(String::as_str).collect();
                names.sort_unstable();
                notes.push(format!("scope: {}", names.join(", ")));
            }
            let note = if notes.is_empty() { String::new() } else {
                format!("  {}", notes.join("  ·  ").dimmed())
            };
            let practice_count = skills.iter()
                .filter(|s| s.category == crate::skill_manager::SkillCategory::Practice)
                .count();
            let domain_count = skills.iter()
                .filter(|s| s.category == crate::skill_manager::SkillCategory::Domain)
                .count();
            println!(
                "  {} {} skill(s): {} core · {} practice · {} domain{}",
                "◎".truecolor(255, 200, 60),
                skills.len().to_string().cyan(),
                always_on.len(),
                practice_count,
                domain_count,
                note,
            );
        }

        let hooks = crate::hooks::HookRunner::load();
        if !hooks.is_empty() && !config.is_subagent && !config.tui_mode {
            println!(
                "  {} {} hook(s) loaded",
                "◎".truecolor(255, 160, 80),
                hooks.total().to_string().cyan(),
            );
        }

        if mcp_server_count > 0 && !config.is_subagent && !config.tui_mode {
            let mut server_names: Vec<&str> = tools.pending_mcp_servers()
                .iter()
                .map(|(n, _)| *n)
                .collect();
            server_names.sort_unstable();
            println!(
                "  {} {} MCP server(s) available (lazy): {}",
                "⬡".truecolor(255, 140, 60),
                server_names.len().to_string().cyan(),
                server_names.join(", ").dimmed(),
            );
        } else if mcp_had_config && !config.is_subagent && !config.tui_mode {
            println!(
                "  {} {}",
                "○".truecolor(180, 120, 60),
                "MCP config found but no runnable stdio servers — all entries are disabled or use SSE/HTTP transport  (/mcp to edit)".truecolor(150, 120, 80),
            );
        }

        if !config.is_subagent && !config.tui_mode {
            if let Some(summary) = crate::http::network_summary(config) {
                println!(
                    "  {} {}",
                    "◎".truecolor(180, 180, 100),
                    summary.dimmed(),
                );
            }
        }

        // ── C1: Session context banner + system prompt injection ──────────────
        let mut startup_notices: Vec<String> = Vec::new();
        if !config.is_subagent {
            if let Some(summary) = crate::project::context_summary() {
                let files = crate::project::context_files();
                let files_part = if files.is_empty() {
                    String::new()
                } else {
                    format!("Files: {}", files.join(", "))
                };

                if config.tui_mode {
                    // TUI: silently inject + queue notices for welcome area.
                    startup_notices.push(format!("↩ Last: {}", summary));
                    if !files_part.is_empty() {
                        startup_notices.push(format!("   {}", files_part));
                    }
                    if let Some(ctx) = crate::project::load_session_context() {
                        system.push_str("\n\n## Last Session Handoff\n");
                        system.push_str(&ctx);
                    }
                } else {
                    // CLI: show banner and ask whether to resume (TTY only).
                    println!("  {} Last: {}", "◌".dimmed(), summary.truecolor(180, 175, 210));
                    if !files_part.is_empty() {
                        println!("  {} {}", "◌".dimmed(), files_part.dimmed());
                    }
                    let is_tty = unsafe { libc::isatty(0 as libc::c_int) } != 0;
                    let resume = if is_tty {
                        Confirm::new("Resume from last session?")
                            .with_default(true)
                            .prompt()
                            .unwrap_or(false)
                    } else {
                        false
                    };
                    if resume {
                        if let Some(ctx) = crate::project::load_session_context() {
                            system.push_str("\n\n## Last Session Handoff\n");
                            system.push_str(&ctx);
                        }
                    }
                }
            }

            // C3 nudge: print for CLI, queue for TUI.
            if let Some(nudge) = index_nudge {
                if config.tui_mode {
                    startup_notices.push(nudge);
                } else {
                    println!("  {} {}", "◌".dimmed(), nudge);
                }
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

        // Refresh understanding.md with deterministic facts at session start.
        // Uses cached index stats (no re-scan) so startup stays fast.
        if !config.is_subagent {
            let (files, symbols, langs) = code_index.lock().ok().and_then(|guard| {
                let (f, s) = guard.total_stats().ok()?;
                let l = guard.stats_by_language().ok().unwrap_or_default();
                Some((f, s, l))
            }).unwrap_or_default();
            let cwd_name = cwd.file_name().map(|n| n.to_string_lossy().to_string());
            if let Err(e) = crate::project::refresh_understanding_md(cwd_name, files, symbols, &langs) {
                crate::log::write("WARN ", &format!("could not refresh understanding.md: {e}"));
            }
        }

        // Spawn background tree-sitter indexer for interactive sessions.
        if !config.is_subagent {
            crate::code_index::spawn_background_indexer(cwd.clone());
        }

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
            domain_scope,
            pinned_skills: std::collections::HashSet::new(),
            current_branch: "main".to_string(),
            code_index,
            store,
            hooks,
            thinking_budget: 0,
            compact_failures: 0,
            files_changed: Vec::new(),
            startup_notices,
            skill_trace: Vec::new(),
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
                ContentBlock::Reasoning { content, .. }  => content.len() / 4,
            }).sum::<usize>()
        }).sum();
        chars / 4
    }

    /// Context fill as 0–100 percentage.
    /// Priority: ZAP_MAX_CONTEXT_TOKENS env var → --budget flag → model default.
    pub fn context_fill_pct(&self) -> u8 {
        let tokens = self.estimated_context_tokens();
        let limit = std::env::var("ZAP_MAX_CONTEXT_TOKENS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .or_else(|| self.config.budget.map(|b| b as usize))
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
        // DISABLE_COMPACT=1 turns off all automatic compaction (use for debugging).
        let disable_compact = std::env::var("DISABLE_COMPACT").is_ok();
        let ctx_pct = self.context_fill_pct();
        let ctx_limit_k = std::env::var("ZAP_MAX_CONTEXT_TOKENS")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .or_else(|| self.config.budget.map(|b| b as usize))
            .unwrap_or_else(|| model_context_limit(&self.model)) / 1000;
        let ctx_used_k = (self.estimated_context_tokens() / 1000).max(1);

        // --budget hard stop: refuse new turns when at 100%.
        if self.config.budget.is_some() && ctx_pct >= 100 {
            println!(
                "  {} Token budget exhausted (~{}k tokens). Start a new session or use /compact.",
                "✗".red().bold(), ctx_used_k
            );
            return Ok(());
        }
        // Silent auto-compact at 90%+ — no blocking dialog. Circuit breaker stops after
        // 3 failures so autonomous tasks don't loop forever on an uncompactable session.
        if !disable_compact && ctx_pct >= 90 && self.compact_failures < 3 {
            println!(
                "  {} Context {}% (~{}k/{}k) — compacting…",
                "⟳".truecolor(200, 150, 60), ctx_pct, ctx_used_k, ctx_limit_k,
            );
            self.cmd_compact().await;
        }

        // Determine once whether this is a no-context casual turn.
        // A message that looks casual but is answering a question or confirming
        // an action (e.g. "ok push it", "yes", "go ahead") is NOT casual.
        let is_casual = is_casual_message(input) && !needs_prior_context(input, &self.messages);

        // Skip skill injection entirely for casual/greeting messages — saves 3-10k tokens.
        let matched_skills: Vec<&crate::skill_manager::Skill> = if is_casual {
            Vec::new()
        } else {
            let mut ms = crate::skill_manager::match_skills_scoped(input, &self.skills, &self.domain_scope);
            // Inject explicitly pinned skills every turn regardless of trigger matching.
            for skill in &self.skills {
                if self.pinned_skills.contains(&skill.name)
                    && !ms.iter().any(|s| s.name == skill.name)
                {
                    ms.push(skill);
                }
            }
            ms
        };
        let skill_tokens_this_turn: usize = matched_skills.iter().map(|s| s.tokens()).sum();

        // Record per-turn skill trace for /skill log.
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
            // Append skill block to the already-built base system prompt instead of
            // rebuilding from scratch — avoids re-reading CLAUDE.md on every skill turn.
            format!("{}\n\n{}", self.system, skill_block)
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
            // Casual turns (greetings, acks) will never call a tool — skip the
            // entire tool definitions array to avoid paying ~2k tokens for nothing.
            let effective_tools: &[serde_json::Value] = if is_casual {
                &[]
            } else {
                &turn_tools
            };
            // Casual turns also need no history — a greeting has no use for a
            // 20-turn code exploration. Send only the current user message.
            // Non-casual turns get a pruned, windowed slice of history.
            let effective_msgs_owned: Vec<Message> = if is_casual {
                self.messages.last().cloned().into_iter().collect()
            } else {
                windowed_history(&self.messages)
            };
            let effective_messages: &[Message] = &effective_msgs_owned;
            let result = self.client
                .send(&effective_system, effective_messages, effective_tools, Some(before_output), self.thinking_budget)
                .await;
            spinner.finish_and_clear();

            // Reactive overflow compaction: if the API rejects the request because the
            // prompt is too long, compact history and retry transparently.
            // Also retry on SSE stream errors (connection dropped mid-stream by the server).
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

            // Empty response: two known causes.
            // (a) Zero input_tokens → context window exceeded (server sends 200 OK but empty SSE).
            // (b) Non-zero input_tokens → proxy or gateway dropped the response body.
            if response.content.is_empty() {
                let input_tokens = response.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0);
                if input_tokens == 0 {
                    // Reactive overflow: compact and retry once before giving up.
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

                // Compute cumulative session cost and push to TUI header.
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
            #[derive(Clone)]
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

                let mut perm_decision = self.permissions.quick_check(name);
                // MCP tools aren't in WRITE_TOOLS so quick_check gives Allow in Ask mode.
                // Upgrade to NeedsPrompt unless the user already pressed "always" this session.
                if matches!(perm_decision, crate::permission_manager::QuickDecision::Allow)
                    && self.tools.is_mcp_tool(name)
                    && matches!(self.permissions.mode, crate::config::PermissionMode::Ask)
                    && !self.permissions.is_session_granted(name)
                {
                    perm_decision = crate::permission_manager::QuickDecision::NeedsPrompt;
                }
                match perm_decision {
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
                // In TUI mode the permission dialog renders in-place (raw mode stays on).
                // Only suspend for CLI / inquire prompts that need a full terminal.
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

            // Phase 1b: handle mcp_connect calls (mutates tool registry — must run before parallel phase).
            let mut connect_calls: Vec<ApprovedCall> = Vec::new();
            approved.retain(|c| {
                if c.name == "mcp_connect" { connect_calls.push(c.clone()); false } else { true }
            });
            for call in connect_calls {
                let server_name = call.input["server"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();

                // Emit ToolStart so TUI shows the connecting state.
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
                                    let preview = output.lines().take(10).collect::<Vec<_>>().join("\n");
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
                                    const MAX_TOOL_BYTES: usize = 20_000;
                                    let content = if output.len() > MAX_TOOL_BYTES {
                                        // Walk back to a valid UTF-8 char boundary.
                                        let mut cut = MAX_TOOL_BYTES;
                                        while cut > 0 && !output.is_char_boundary(cut) {
                                            cut -= 1;
                                        }
                                        format!(
                                            "{}\n\n[... truncated — output was {} bytes, showing first {}]",
                                            &output[..cut],
                                            output.len(),
                                            cut,
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

            // Reindex and record any files that tools reported they wrote to.
            for block in &tool_calls {
                if let ContentBlock::ToolUse { name, input, .. } = block {
                    if let Some(tool) = self.tools.get(name) {
                        if let Some(path_str) = tool.affected_path(input) {
                            crate::code_index::global_reindex_file(std::path::Path::new(path_str));
                            self.files_changed.push(path_str.to_string());
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
                            let send_anyway = if crate::tui::channel::is_tui_mode() {
                                // TUI-native path: async-await so the tick loop stays alive.
                                let (tx, rx) = tokio::sync::oneshot::channel();
                                crate::tui::channel::set_secret_request(
                                    crate::tui::channel::SecretScannerRequest {
                                        hits: hits.iter().map(|h| h.to_string()).collect(),
                                        response_tx: tx,
                                    },
                                );
                                rx.await.unwrap_or(false)
                            } else {
                                // CLI path: suspend terminal, prompt, resume.
                                crate::tui::channel::suspend_for_prompt();
                                println!("  {} possible secret(s) detected before cloud send:", "⚠".yellow().bold());
                                for h in &hits { println!("    {}", h.to_string().yellow()); }
                                print!("  send anyway? [y/N] ");
                                let _ = std::io::Write::flush(&mut std::io::stdout());
                                let mut ans = String::new();
                                std::io::stdin().read_line(&mut ans).ok();
                                let send = ans.trim().eq_ignore_ascii_case("y");
                                if !send {
                                    println!("  {} aborted by user — secrets not sent", "✗".red());
                                }
                                crate::tui::channel::resume_from_prompt();
                                send
                            };
                            if !send_anyway {
                                if crate::tui::channel::is_tui_mode() {
                                    crate::tui::channel::tui_send(
                                        crate::tui::channel::TuiEvent::LlmChunk(
                                            "\n⚠ Secrets detected in tool output — turn cancelled.".to_string(),
                                        ),
                                    );
                                }
                                return Ok(());
                            }
                        }
                    }
                }
            }

            // Inject any mid-turn btw messages the user typed via Ctrl+B.
            let btw_msgs = crate::tui::channel::drain_btw();
            let mut tool_msg = Message::tool_results(tool_results);
            if !btw_msgs.is_empty() {
                let note = btw_msgs
                    .iter()
                    .map(|m| format!("↳ User note (added mid-turn): {m}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                // Append as a text block inside the user message so the model sees it
                // in context without starting a new turn.
                if let Some(block) = tool_msg.content.last_mut() {
                    if let ContentBlock::Text { text } = block {
                        text.push_str(&format!("\n\n{note}"));
                    } else {
                        tool_msg.content.push(ContentBlock::Text { text: note });
                    }
                } else {
                    tool_msg.content.push(ContentBlock::Text { text: note });
                }
            }
            self.messages.push(tool_msg);
        }

        // Persist conversation after every turn.
        if let Ok(json) = serde_json::to_string(&self.messages) {
            let _ = self.store.save_messages(self.session_id, &json);
        }

        // Signal remote control clients that the turn is complete.
        crate::remote_channel::send_done();

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
            "/compact"     => { self.cmd_compact().await; }
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
            "/remote"      => {
                let port: u16 = arg.parse().unwrap_or(0);
                crate::remote_channel::activate();
                match crate::remote::start_server(port).await {
                    Ok(actual_port) => {
                        println!("  {} remote server on http://127.0.0.1:{}", "⚡".bright_yellow(), actual_port);
                        match crate::remote::launch_tunnel(actual_port).await {
                            Ok(url) => {
                                println!("  {} {}", "🌐".truecolor(100, 200, 255), url.cyan().bold());
                                println!("     Open on any device — type messages, get responses in real time.");
                            }
                            Err(e) => println!("  {} tunnel failed: {} — use local URL on same network", "⚠".yellow(), e),
                        }
                    }
                    Err(e) => println!("  {} {}", "✗".red(), e),
                }
            }
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
            "/deploy"      => self.cmd_deploy(arg).await,
            "/exit" | "/quit" => return true,
            other => println!("  {} Unknown command {}. Try {}.",
                "✗".red(), other.yellow(), "/help".cyan()),
        }
        false
    }
}

/// Returns true when the message is a casual greeting or short social phrase
/// where injecting skills/tool context would waste tokens with no benefit.
/// Call site must also check `needs_prior_context` before treating as no-history.
fn is_casual_message(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    // Long messages are never casual.
    if t.len() > 80 { return false; }
    // Any technical keywords → not casual.
    let technical = [
        // code actions
        "fix", "bug", "code", "file", "function", "error", "test",
        "create", "add", "change", "update", "delete", "remove",
        "build", "run", "compile", "refactor", "write", "read",
        "show me", "explain", "how do", "what is", "why is",
        "implement", "debug", "check", "review", "edit", "find",
        "search", "list", "open", "close", "move", "rename",
        // git / ops — previously missing
        "push", "pull", "commit", "merge", "deploy", "release",
        "install", "revert", "reset", "branch", "checkout", "clone",
        "diff", "log", "stash", "tag", "patch",
    ];
    if technical.iter().any(|kw| t.contains(kw)) { return false; }
    // Positive match: starts with or equals a greeting pattern.
    let greetings = ["hi", "hello", "hey", "howdy", "greetings", "sup", "yo",
                     "how are you", "what's up", "whats up", "good morning",
                     "good evening", "good afternoon", "good night",
                     "thanks", "thank you", "ty", "thx",
                     "ok", "okay", "sure", "great", "nice", "cool", "awesome",
                     "sounds good", "perfect", "got it", "makes sense",
                     "what can you do", "what do you do"];
    greetings.iter().any(|g| t == *g
        || t.starts_with(&format!("{} ", g))
        || t.starts_with(&format!("{},", g))
        || t.starts_with(&format!("{}!", g)))
}

/// Returns true when the user's message is confirming or continuing a pending
/// action — e.g. "yes", "go ahead", "do it". Even if the text looks casual,
/// these replies need conversation history to be meaningful.
fn is_action_confirmation(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    let confirmations = [
        "yes", "yeah", "yep", "yup", "y",
        "no", "nope", "nah", "n",
        "do it", "go ahead", "go for it", "proceed",
        "let's go", "lets go", "let's do it", "lets do it",
        "continue", "keep going", "carry on",
    ];
    confirmations.iter().any(|c| t == *c
        || t.starts_with(&format!("{} ", c))
        || t.starts_with(&format!("{},", c))
        || t.starts_with(&format!("{}!", c)))
}

/// Returns true when the last assistant message asked the user a question,
/// meaning the user's next reply is an answer and needs full history context.
fn last_message_was_question(messages: &[Message]) -> bool {
    messages.iter().rev()
        .find(|m| m.role == "assistant")
        .map(|m| {
            // Check text blocks in the last assistant message for a question mark.
            m.content.iter().any(|b| {
                if let ContentBlock::Text { text } = b {
                    let trimmed = text.trim_end_matches(|c: char| c.is_whitespace() || c == '*');
                    trimmed.ends_with('?')
                } else {
                    false
                }
            })
        })
        .unwrap_or(false)
}

/// Returns true when the message, despite appearing casual, needs prior
/// conversation history to be answered correctly.
///
/// Pure greetings (hi, hello, hey, thanks…) are NEVER considered to need
/// prior context — even if the model's last message ended with "?".
/// A user saying "hi" again mid-session is always a greeting, not an answer.
/// Only action-confirmations (yes/no/proceed/go ahead) that are replying to
/// a question get full history injected.
fn needs_prior_context(text: &str, messages: &[Message]) -> bool {
    if is_action_confirmation(text) {
        return true;
    }
    // Only treat last-message-was-question as requiring context when the
    // reply could plausibly be an answer — not for bare greetings.
    if last_message_was_question(messages) && !is_pure_greeting(text) {
        return true;
    }
    false
}

/// True when the text is a bare greeting or social phrase that can never be
/// an answer to a question: "hi", "hello", "hey", "thanks", "cool", etc.
fn is_pure_greeting(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    let greetings = ["hi", "hello", "hey", "howdy", "yo", "sup",
                     "thanks", "thank you", "ty", "thx",
                     "good morning", "good evening", "good afternoon", "good night",
                     "how are you", "what's up", "whats up",
                     "what can you do", "what do you do"];
    greetings.iter().any(|g| t == *g
        || t.starts_with(&format!("{} ", g))
        || t.starts_with(&format!("{},", g))
        || t.starts_with(&format!("{}!", g)))
}

/// Build the message slice to send for a non-casual turn:
///
/// 1. **Sliding window** — only the last `ZAP_HISTORY_WINDOW` real user turns
///    (default 8) are included, bounding token cost regardless of session length.
///
/// 2. **Tool-result pruning** — ToolResult blocks outside the last 2 complete
///    exchanges are replaced with a one-line stub. The model already incorporated
///    that content into its previous reply; keeping it verbatim is pure noise.
fn windowed_history(messages: &[Message]) -> Vec<Message> {
    let window: usize = std::env::var("ZAP_HISTORY_WINDOW")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);

    // Identify indices of "real" user turns (Text-first, not tool-result turns).
    let real_turn_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            m.role == "user"
                && m.content
                    .first()
                    .map_or(false, |b| matches!(b, ContentBlock::Text { .. }))
        })
        .map(|(i, _)| i)
        .collect();

    // Window start: Nth-last real user turn, or the beginning.
    let start = if real_turn_indices.len() > window {
        real_turn_indices[real_turn_indices.len() - window]
    } else {
        0
    };

    // Prune tool results that are outside the last 2 complete exchanges — they
    // have already been processed and only inflate the prompt.
    let prune_before = if real_turn_indices.len() > 2 {
        real_turn_indices[real_turn_indices.len() - 2]
    } else {
        0 // nothing to prune yet
    };

    const PRUNE_THRESHOLD: usize = 300; // chars; small results keep full fidelity

    messages[start..]
        .iter()
        .enumerate()
        .map(|(rel_i, msg)| {
            let abs_i = start + rel_i;
            if abs_i < prune_before {
                // Replace oversized ToolResult content with a compact stub.
                let pruned: Vec<ContentBlock> = msg
                    .content
                    .iter()
                    .map(|block| match block {
                        ContentBlock::ToolResult { tool_use_id, content }
                            if content.len() > PRUNE_THRESHOLD =>
                        {
                            ContentBlock::ToolResult {
                                tool_use_id: tool_use_id.clone(),
                                content: format!("[pruned — {} chars]", content.len()),
                            }
                        }
                        other => other.clone(),
                    })
                    .collect();
                Message { role: msg.role.clone(), content: pruned }
            } else {
                msg.clone()
            }
        })
        .collect()
}

#[cfg(test)]
mod casual_tests {
    use super::is_casual_message;

    // ── Should match (casual) ─────────────────────────────────────────────────

    #[test]
    fn bare_greetings() {
        for msg in &["hi", "Hello", "HEY", "howdy", "yo", "sup"] {
            assert!(is_casual_message(msg), "{msg:?} should be casual");
        }
    }

    #[test]
    fn greeting_with_trailing_text() {
        assert!(is_casual_message("hi there"));
        assert!(is_casual_message("hello, world"));
        assert!(is_casual_message("hey!"));
        assert!(is_casual_message("good morning everyone"));
    }

    #[test]
    fn acknowledgements() {
        for msg in &["ok", "okay", "sure", "great", "thanks", "thank you", "ty",
                     "thx", "cool", "awesome", "sounds good", "perfect",
                     "got it", "makes sense"] {
            assert!(is_casual_message(msg), "{msg:?} should be casual");
        }
    }

    #[test]
    fn capability_question() {
        assert!(is_casual_message("what can you do"));
        assert!(is_casual_message("what do you do"));
    }

    #[test]
    fn mixed_case_and_whitespace() {
        assert!(is_casual_message("  Hi  "));
        assert!(is_casual_message("THANKS"));
        assert!(is_casual_message("Hey there!"));
    }

    // ── Should NOT match (technical) ─────────────────────────────────────────

    #[test]
    fn technical_keywords_block_casual() {
        let cases = [
            "hi, can you fix this bug",
            "hey, show me the code",
            "hello, how do I build this",
            "ok, what is the error",
            "sure, create a test",
            "great, can you add a function",
        ];
        for msg in &cases {
            assert!(!is_casual_message(msg), "{msg:?} should NOT be casual");
        }
    }

    #[test]
    fn long_message_never_casual() {
        let long = "hi ".repeat(30); // > 80 chars
        assert!(!is_casual_message(&long));
    }

    #[test]
    fn technical_standalone() {
        for msg in &["fix the login bug", "refactor this module",
                     "write a test", "explain this function", "find the error"] {
            assert!(!is_casual_message(msg), "{msg:?} should NOT be casual");
        }
    }

    #[test]
    fn not_a_known_greeting_prefix() {
        assert!(!is_casual_message("random stuff"));
        assert!(!is_casual_message("welcome back"));
        assert!(!is_casual_message("morning"));
    }

    // ── Git/ops keywords now block casual ─────────────────────────────────────

    #[test]
    fn git_ops_block_casual() {
        let cases = [
            "ok push it",
            "sure, pull",
            "great, commit now",
            "ok deploy",
            "nice, merge it",
            "cool, revert that",
            "sure reset",
        ];
        for msg in &cases {
            assert!(!is_casual_message(msg), "{msg:?} should NOT be casual");
        }
    }
}

#[cfg(test)]
mod prior_context_tests {
    use super::{needs_prior_context, is_pure_greeting};
    use crate::llm_client::{Message, ContentBlock};

    fn assistant_msg(text: &str) -> Message {
        Message { role: "assistant".to_string(), content: vec![ContentBlock::Text { text: text.to_string() }] }
    }

    // ── Pure greetings are NEVER context-dependent ────────────────────────────

    #[test]
    fn hi_after_question_stays_casual() {
        // Regression: second "hi" in a session was getting full context because
        // the model's previous reply ("How can I help you today?") ended with "?".
        let history = vec![
            Message::user_text("hi"),
            assistant_msg("Hello! How can I help you today?"),
        ];
        assert!(!needs_prior_context("hi", &history));
        assert!(!needs_prior_context("hello", &history));
        assert!(!needs_prior_context("hey", &history));
        assert!(!needs_prior_context("thanks", &history));
    }

    #[test]
    fn hi_with_no_history_not_context_dependent() {
        assert!(!needs_prior_context("hi", &[]));
    }

    // ── Action confirmations always need context ──────────────────────────────

    #[test]
    fn yes_after_question_needs_context() {
        let history = vec![
            Message::user_text("refactor auth"),
            assistant_msg("Should I also update the tests?"),
        ];
        assert!(needs_prior_context("yes", &history));
        assert!(needs_prior_context("go ahead", &history));
        assert!(needs_prior_context("proceed", &history));
    }

    #[test]
    fn yes_without_question_still_needs_context() {
        // Confirmations always need history — they're action-driven.
        assert!(needs_prior_context("yes", &[]));
        assert!(needs_prior_context("no", &[]));
    }

    // ── Non-greeting short replies need context when model asked a question ───

    #[test]
    fn short_answer_after_question_needs_context() {
        let history = vec![
            Message::user_text("fix the bug"),
            assistant_msg("Which file should I start with?"),
        ];
        // "main.rs" is not a greeting — it's an answer
        assert!(needs_prior_context("main.rs", &history));
        assert!(needs_prior_context("the second one", &history));
    }

    // ── is_pure_greeting coverage ─────────────────────────────────────────────

    #[test]
    fn pure_greetings_recognised() {
        for g in &["hi", "hello", "hey", "thanks", "thank you", "good morning"] {
            assert!(is_pure_greeting(g), "{g:?} should be a pure greeting");
        }
    }

    #[test]
    fn technical_text_not_pure_greeting() {
        assert!(!is_pure_greeting("main.rs"));
        assert!(!is_pure_greeting("yes"));
        assert!(!is_pure_greeting("the auth module"));
    }
}

#[cfg(test)]
mod context_tests {
    use super::{is_action_confirmation, last_message_was_question};
    use crate::llm_client::{ContentBlock, Message};

    #[test]
    fn action_confirmations_detected() {
        for msg in &["yes", "no", "y", "n", "do it", "go ahead", "proceed",
                     "continue", "let's go", "go for it"] {
            assert!(is_action_confirmation(msg), "{msg:?} should be action confirmation");
        }
    }

    #[test]
    fn social_words_not_confirmations() {
        for msg in &["thanks", "hi", "hello", "great", "cool", "amazing"] {
            assert!(!is_action_confirmation(msg), "{msg:?} should NOT be action confirmation");
        }
    }

    #[test]
    fn detects_question_in_last_assistant_message() {
        let messages = vec![
            Message { role: "user".to_string(), content: vec![ContentBlock::Text { text: "help".to_string() }] },
            Message { role: "assistant".to_string(), content: vec![ContentBlock::Text { text: "Should I push to main?".to_string() }] },
        ];
        assert!(last_message_was_question(&messages));
    }

    #[test]
    fn no_false_positive_when_no_question() {
        let messages = vec![
            Message { role: "user".to_string(), content: vec![ContentBlock::Text { text: "help".to_string() }] },
            Message { role: "assistant".to_string(), content: vec![ContentBlock::Text { text: "Done, pushed to main.".to_string() }] },
        ];
        assert!(!last_message_was_question(&messages));
    }
}
