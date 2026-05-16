use anyhow::Result;
use colored::Colorize;
use futures::future::join_all;
use std::io::{self, Write as IoWrite};

use crate::{
    audit,
    config::Config,
    context_manager,
    llm_client::{create_client, ContentBlock, LlmProvider, Message},
    permission_manager::PermissionManager,
    persistence,
    tool_registry::ToolRegistry,
};

const MAX_TURNS: usize = 50;

// ── Session ───────────────────────────────────────────────────────────────────

struct Session {
    client: Box<dyn LlmProvider>,
    tools: ToolRegistry,
    permissions: PermissionManager,
    system: String,
    tool_defs: Vec<serde_json::Value>,
    messages: Vec<Message>,
    model: String,
    base_url: Option<String>,
}

impl Session {
    async fn new(config: &Config) -> Result<Self> {
        let store = persistence::init()?;
        let _ = store.save_session("(repl)", &config.model)?;

        let system = context_manager::build_system_prompt(config)?;
        let tools = ToolRegistry::new();
        let tool_defs = tools.tool_definitions();

        Ok(Self {
            client: create_client(config),
            tools,
            permissions: PermissionManager::new(config.permission_mode.clone()),
            system,
            tool_defs,
            messages: Vec::new(),
            model: config.model.clone(),
            base_url: config.base_url.clone(),
        })
    }

    /// Drive the inner tool loop for a single user turn.
    async fn handle_user_turn(&mut self, input: &str) -> Result<()> {
        self.messages.push(Message::user_text(input));
        audit::record(&format!("user_turn: {}", input))?;

        for turn in 0..MAX_TURNS {
            tracing::info!(turn = turn, "calling LLM");
            let response = self
                .client
                .send(&self.system, &self.messages, &self.tool_defs)
                .await?;
            audit::record(&format!(
                "llm_response turn={} stop_reason={}",
                turn, response.stop_reason
            ))?;

            // Text was already streamed to stdout by the LLM client; do not print again.

            let tool_calls: Vec<&ContentBlock> = response
                .content
                .iter()
                .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
                .collect();

            if tool_calls.is_empty() {
                break;
            }

            self.messages.push(Message {
                role: "assistant".to_string(),
                content: response.content.clone(),
            });

            // ── Phase 1: check all permissions sequentially ───────────────────
            struct ApprovedCall {
                id: String,
                name: String,
                input: serde_json::Value,
                ctx: String,
            }
            let mut approved: Vec<ApprovedCall> = Vec::new();
            let mut tool_results: Vec<ContentBlock> = Vec::new();

            for block in &tool_calls {
                let ContentBlock::ToolUse { id, name, input } = block else {
                    continue;
                };
                tracing::info!(tool = %name, "tool use requested");
                audit::record(&format!("tool_request name={} id={}", name, id))?;

                let ctx = self
                    .tools
                    .get(name)
                    .map(|t| t.permission_context(input))
                    .unwrap_or_default();
                let allowed = self.permissions.check(name, &ctx)?;

                if !allowed {
                    audit::record(&format!("tool_denied name={} id={}", name, id))?;
                    tool_results.push(ContentBlock::ToolResult {
                        tool_use_id: id.clone(),
                        content: "Permission denied by user.".to_string(),
                    });
                } else {
                    approved.push(ApprovedCall {
                        id: id.clone(),
                        name: name.clone(),
                        input: input.clone(),
                        ctx,
                    });
                }
            }

            // ── Phase 2: execute approved tools in parallel ───────────────────
            let exec_futures = approved.into_iter().map(|call| {
                let tool = self.tools.get(&call.name);
                async move {
                    println!(
                        "  {} {}  {}",
                        "●".bright_yellow(),
                        call.name.cyan().bold(),
                        call.ctx.dimmed()
                    );
                    match tool {
                        Some(t) => {
                            let _ = audit::record(&format!(
                                "tool_execute name={} input={}",
                                call.name,
                                serde_json::to_string(&call.input).unwrap_or_default()
                            ));
                            match t.execute(call.input).await {
                                Ok(output) => {
                                    let _ = audit::record(&format!(
                                        "tool_success name={}",
                                        call.name
                                    ));
                                    ContentBlock::ToolResult {
                                        tool_use_id: call.id,
                                        content: output,
                                    }
                                }
                                Err(e) => {
                                    let _ = audit::record(&format!(
                                        "tool_error name={} err={}",
                                        call.name, e
                                    ));
                                    ContentBlock::ToolResult {
                                        tool_use_id: call.id,
                                        content: format!("{} {}", "Error:".red(), e),
                                    }
                                }
                            }
                        }
                        None => {
                            let _ = audit::record(&format!("tool_unknown name={}", call.name));
                            ContentBlock::ToolResult {
                                tool_use_id: call.id,
                                content: format!("Unknown tool: {}", call.name),
                            }
                        }
                    }
                }
            });
            tool_results.extend(join_all(exec_futures).await);

            self.messages.push(Message::tool_results(tool_results));
        }

        Ok(())
    }

    // ── Slash command handlers ────────────────────────────────────────────────

    fn cmd_help(&self) {
        println!();
        println!("  {} {}", "⚡ zap".bright_yellow().bold(), "slash commands".dimmed());
        println!("  {}", "──────────────────────────────────".dimmed());
        let cmds = [
            ("/help",      "Show this help"),
            ("/config",    "Show provider, model, and URL"),
            ("/models",    "List models available on the server"),
            ("/model X",   "Switch to model X for this session"),
            ("/clear",     "Clear conversation history"),
            ("/history",   "Show number of turns in this session"),
            ("/exit",      "Quit"),
        ];
        for (cmd, desc) in cmds {
            println!("  {:<14} {}", cmd.cyan(), desc.dimmed());
        }
        println!();
    }

    fn cmd_config(&self) {
        println!();
        let provider = if self.base_url.is_some() { "openai-compatible" } else { "anthropic" };
        let url = self.base_url.as_deref().unwrap_or("https://api.anthropic.com");
        println!("  {}", "Current configuration".bold());
        println!("  {}", "──────────────────────────────────".dimmed());
        println!("  {:<14} {}", "provider".dimmed(), provider.cyan());
        println!("  {:<14} {}", "model".dimmed(), self.model.cyan().bold());
        println!("  {:<14} {}", "base_url".dimmed(), url.cyan());
        println!();
    }

    fn cmd_history(&self) {
        let turns = self.messages.len();
        println!("  {} messages in history", turns.to_string().cyan());
    }

    fn cmd_clear(&mut self) {
        self.messages.clear();
        println!("  {} History cleared.", "✓".green());
    }

    async fn cmd_models(&self) {
        let url = match &self.base_url {
            Some(b) => format!("{}/v1/models", b.trim_end_matches('/')),
            None => {
                println!("  {} /models only works with OpenAI-compatible servers.", "✗".red());
                return;
            }
        };

        let client = reqwest::Client::new();
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => {
                        println!();
                        println!("  {}", "Available models".bold());
                        println!("  {}", "──────────────────────────────────".dimmed());
                        if let Some(arr) = json["data"].as_array() {
                            for m in arr {
                                let id = m["id"].as_str().unwrap_or("?");
                                let active = if id == self.model { " ◀ active".green().to_string() } else { String::new() };
                                println!("  {} {}{}", "·".dimmed(), id.cyan(), active);
                            }
                        }
                        println!();
                        println!("  {}", "Use /model <id> to switch.".dimmed());
                        println!();
                    }
                    Err(e) => println!("  {} Failed to parse response: {}", "✗".red(), e),
                }
            }
            Ok(resp) => println!("  {} Server returned {}", "✗".red(), resp.status()),
            Err(e) => println!("  {} Could not reach server: {}", "✗".red(), e),
        }
    }

    fn cmd_model(&mut self, name: &str, config: &Config) {
        self.model = name.to_string();
        let mut new_config = config.clone();
        new_config.model = name.to_string();
        self.client = crate::llm_client::create_client(&new_config);
        println!(
            "  {} Switched to {}",
            "✓".green(),
            name.cyan().bold()
        );
    }

    /// Handle a `/command` line. Returns true if the session should end.
    async fn handle_slash(&mut self, line: &str, config: &Config) -> bool {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).copied().unwrap_or("").trim();

        match cmd {
            "/help"    => self.cmd_help(),
            "/config"  => self.cmd_config(),
            "/history" => self.cmd_history(),
            "/clear"   => self.cmd_clear(),
            "/models"  => self.cmd_models().await,
            "/model"   => {
                if arg.is_empty() {
                    println!("  Usage: /model <model-id>");
                } else {
                    self.cmd_model(arg, config);
                }
            }
            "/exit" | "/quit" => return true,
            other => println!(
                "  {} Unknown command {}. Try {}.",
                "✗".red(), other.yellow(), "/help".cyan()
            ),
        }
        false
    }
}

// ── Public entry points ───────────────────────────────────────────────────────

pub async fn run(goal: &str, config: &Config) -> Result<()> {
    audit::record(&format!("session_start goal=\"{}\" model={}", goal, config.model))?;
    let mut session = Session::new(config).await?;
    session.handle_user_turn(goal).await?;
    audit::record("session_end")?;
    Ok(())
}

pub async fn run_repl(config: &Config) -> Result<()> {
    audit::record(&format!("repl_start model={}", config.model))?;
    let mut session = Session::new(config).await?;

    loop {
        print!("\n{} ", "›".bright_yellow().bold());
        io::stdout().flush()?;

        let mut line = String::new();
        let n = io::stdin().read_line(&mut line)?;
        if n == 0 {
            break; // EOF (Ctrl+D)
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        if input.starts_with('/') {
            if session.handle_slash(input, config).await {
                break;
            }
            continue;
        }

        if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
            break;
        }

        if let Err(e) = session.handle_user_turn(input).await {
            eprintln!("  {} {}", "Error:".red().bold(), e);
        }
    }

    println!("\n  {} Goodbye.", "⚡".bright_yellow());
    audit::record("repl_end")?;
    Ok(())
}
