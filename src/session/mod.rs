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
const AUTO_COMPACT_THRESHOLD: usize = 80_000;

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
}

impl Session {
    pub async fn new(config: &Config) -> Result<Self> {
        let store = persistence::init()?;
        let session_id = store.save_session("(repl)", &config.model)?;

        let mut system = context_manager::build_system_prompt(config)?;
        let mut tools = ToolRegistry::new();
        tools.register_mcp_servers().await;
        if config.agent_depth > 0 {
            tools.register(std::sync::Arc::new(SpawnAgentTool::new(config.clone())));
        }
        let tool_defs  = tools.tool_definitions();
        let tool_count = tool_defs.len();

        let skills      = crate::skill_manager::load_all_skills();
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
            println!(
                "  {} {} skill(s) loaded{}",
                "◎".truecolor(255, 200, 60),
                skills.len().to_string().cyan(),
                note,
            );
        }

        let hooks = crate::hooks::HookRunner::load();
        if !hooks.is_empty() {
            println!(
                "  {} {} hook(s) loaded",
                "◎".truecolor(255, 160, 80),
                hooks.total().to_string().cyan(),
            );
        }

        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let code_index = match crate::code_index::CodeIndex::open(&cwd) {
            Ok(mut idx) => {
                match idx.index_dir(&cwd) {
                    Ok((0, _)) => {}
                    Ok((files, syms)) => {
                        println!(
                            "  {} indexed {} file(s), {} symbol(s)",
                            "◎".truecolor(100, 200, 255),
                            files.to_string().cyan(),
                            syms.to_string().cyan(),
                        );
                    }
                    Err(e) => tracing::warn!("code index: {}", e),
                }
                let arc = Arc::new(Mutex::new(idx));
                crate::code_index::set_global(arc.clone());
                arc
            }
            Err(e) => {
                tracing::warn!("code index unavailable: {}", e);
                Arc::new(Mutex::new(
                    crate::code_index::CodeIndex::open(&cwd)
                        .unwrap_or_else(|_| {
                            crate::code_index::CodeIndex::open(std::path::Path::new("/tmp")).unwrap()
                        }),
                ))
            }
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
        })
    }

    pub fn make_spinner() -> ThinkingSpinner { ThinkingSpinner::new() }

    pub fn estimated_context_tokens(&self) -> usize {
        let chars: usize = self.messages.iter().map(|m| {
            m.content.iter().map(|b| match b {
                ContentBlock::Text { text }           => text.len(),
                ContentBlock::ToolUse { input, .. }   => input.to_string().len(),
                ContentBlock::ToolResult { content, .. } => content.len(),
                ContentBlock::Image { data, .. }      => data.len() / 4,
            }).sum::<usize>()
        }).sum();
        chars / 4
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

        let est = self.estimated_context_tokens();
        if est > AUTO_COMPACT_THRESHOLD {
            println!(
                "  {} Context is large (~{}k tokens), auto-compacting…",
                "⚡".bright_yellow(), est / 1000
            );
            self.cmd_compact().await;
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

            let mut spinner = Self::make_spinner();
            let pb_clone    = spinner.pb_clone();
            let stop_clone  = spinner.stop_signal();
            let model_label = self.model.clone();
            let before_output: BeforeOutput = Box::new(move || {
                stop_clone.store(true, Ordering::Relaxed);
                pb_clone.finish_and_clear();
                println!("  {} {}",
                    "╭─".truecolor(70, 65, 90),
                    model_label.truecolor(100, 95, 130));
            });

            let result = self.client
                .send(&effective_system, &self.messages, &self.tool_defs, Some(before_output))
                .await;
            spinner.finish_and_clear();
            let response = result?;

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
                let ctx_t = self.estimated_context_tokens();
                parts.push(format!("ctx ~{}k", (ctx_t / 1000).max(1)));

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
            }

            audit::record(&format!(
                "llm_response turn={} stop_reason={}", turn, response.stop_reason
            ))?;

            let tool_calls: Vec<&ContentBlock> = response.content.iter()
                .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                .collect();

            if tool_calls.is_empty() { break; }

            self.messages.push(Message {
                role:    "assistant".to_string(),
                content: response.content.clone(),
            });

            // Phase 1: check permissions sequentially.
            struct ApprovedCall {
                id:    String,
                name:  String,
                input: serde_json::Value,
                ctx:   String,
            }
            let mut approved:     Vec<ApprovedCall>   = Vec::new();
            let mut tool_results: Vec<ContentBlock>   = Vec::new();

            for block in &tool_calls {
                let ContentBlock::ToolUse { id, name, input } = block else { continue };
                tracing::info!(tool = %name, "tool use requested");
                audit::record(&format!("tool_request name={} id={}", name, id))?;

                let ctx = self.tools.get(name)
                    .map(|t| t.permission_context(input))
                    .unwrap_or_default();
                let allowed = self.permissions.check(name, &ctx)?;

                if !allowed {
                    audit::record(&format!("tool_denied name={} id={}", name, id))?;
                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content:     "Permission denied by user.".to_string(),
                    });
                } else {
                    // Fire PreToolUse hooks — exit code 2 blocks execution.
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
                                id:    id.clone(),
                                name:  name.clone(),
                                input: input.clone(),
                                ctx,
                            });
                        }
                    }
                }
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
                    let ctx_display = if call.ctx.len() > 52 {
                        format!("{}…", &call.ctx[..51])
                    } else {
                        call.ctx.clone()
                    };
                    println!(
                        "  {} {} {}  {}{}",
                        "╭─".truecolor(70, 65, 90),
                        icon,
                        call.name.truecolor(100, 210, 255).bold(),
                        ctx_display.truecolor(130, 120, 155),
                        cancel_hint,
                    );
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
                                    println!("  {} {}  {}",
                                        "╰─".truecolor(70, 65, 90),
                                        "✓".truecolor(80, 210, 120),
                                        format!("{}ms", ms).truecolor(90, 85, 110));
                                    ContentBlock::ToolResult { tool_use_id: call.id, content: output }
                                }
                                Err(e) => {
                                    let _ = audit::record(&format!("tool_error name={} err={}", call.name, e));
                                    let ms = t0.elapsed().as_millis();
                                    println!("  {} {}  {}",
                                        "╰─".truecolor(70, 65, 90),
                                        "✗".truecolor(220, 80, 80),
                                        format!("{}ms", ms).truecolor(90, 85, 110));
                                    ContentBlock::ToolResult {
                                        tool_use_id: call.id,
                                        content:     format!("{} {}", "Error:", e),
                                    }
                                }
                            }
                        }
                        None => {
                            let _ = audit::record(&format!("tool_unknown name={}", call.name));
                            println!("  {} {} unknown tool",
                                "╰─".truecolor(70, 65, 90), "✗".truecolor(220, 80, 80));
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
                            println!("  {} possible secret(s) detected before cloud send:", "⚠".yellow().bold());
                            for h in &hits { println!("    {}", h.to_string().yellow()); }
                            print!("  send anyway? [y/N] ");
                            let _ = std::io::Write::flush(&mut std::io::stdout());
                            let mut ans = String::new();
                            std::io::stdin().read_line(&mut ans).ok();
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
            "/tasks"       => self.cmd_tasks().await,
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
