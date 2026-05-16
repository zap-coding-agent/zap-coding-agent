use anyhow::Result;
use colored::Colorize;
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use rustyline::{
    Editor, Helper,
    completion::{Completer, Pair},
    error::ReadlineError,
    highlight::Highlighter,
    hint::Hinter,
    history::DefaultHistory,
    validate::{ValidationContext, ValidationResult, Validator},
};
use std::borrow::Cow;

use crate::{
    audit,
    config::{Config, PermissionMode},
    context_manager,
    llm_client::{create_client, BeforeOutput, ContentBlock, LlmProvider, Message, Usage},
    permission_manager::PermissionManager,
    persistence,
    tool_registry::ToolRegistry,
};

const MAX_TURNS: usize = 50;
const HISTORY_PATH: &str = ".zap_history";

// ── Rustyline helper: tab-complete slash commands + inline hints ───────────────

const SLASH_COMMANDS: &[&str] = &[
    "/help", "/config", "/models", "/model ",
    "/permissions ", "/clear", "/compact",
    "/history", "/sessions", "/cost",
    "/memory list", "/memory get ", "/memory set ", "/memory del ",
    "/audit", "/exit",
];

struct ZapHelper;

impl Helper for ZapHelper {}
impl Validator for ZapHelper {
    fn validate(&self, _ctx: &mut ValidationContext) -> rustyline::Result<ValidationResult> {
        Ok(ValidationResult::Valid(None))
    }
}

impl Completer for ZapHelper {
    type Candidate = Pair;
    fn complete(
        &self, line: &str, _pos: usize, _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        if !line.starts_with('/') {
            return Ok((0, vec![]));
        }
        let matches = SLASH_COMMANDS.iter()
            .filter(|c| c.starts_with(line))
            .map(|c| Pair { display: c.to_string(), replacement: c.to_string() })
            .collect();
        Ok((0, matches))
    }
}

impl Hinter for ZapHelper {
    type Hint = String;
    fn hint(&self, line: &str, _pos: usize, _ctx: &rustyline::Context<'_>) -> Option<String> {
        if line.len() < 2 || !line.starts_with('/') {
            return None;
        }
        SLASH_COMMANDS.iter()
            .find(|c| c.starts_with(line) && **c != line)
            .map(|c| c[line.len()..].to_string())
    }
}

impl Highlighter for ZapHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if line.starts_with('/') {
            Cow::Owned(format!("\x1b[36m{}\x1b[0m", line)) // cyan
        } else {
            Cow::Borrowed(line)
        }
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[2m{}\x1b[0m", hint)) // dim grey
    }
    fn highlight_char(&self, line: &str, _pos: usize, _forced: bool) -> bool {
        line.starts_with('/')
    }
}

// ── Approximate cost table (USD per million tokens) ──────────────────────────
//   model prefix → (input_$/M, output_$/M)
fn cost_per_million(model: &str) -> (f64, f64) {
    if model.contains("opus-4") {
        (15.0, 75.0)
    } else if model.contains("sonnet-4") {
        (3.0, 15.0)
    } else if model.contains("haiku-4") {
        (0.8, 4.0)
    } else {
        // Unknown / local model — show tokens only
        (0.0, 0.0)
    }
}

fn format_cost(usage: &Usage, model: &str) -> String {
    let (cost_in, cost_out) = cost_per_million(model);
    let input_k  = usage.input_tokens as f64 / 1_000.0;
    let output_k = usage.output_tokens as f64 / 1_000.0;
    let total_usd = (usage.input_tokens as f64 * cost_in
        + usage.output_tokens as f64 * cost_out)
        / 1_000_000.0;

    let cache_note = if usage.cache_read_tokens > 0 {
        format!(
            "  {}  cache hit {}k / write {}k",
            "●".bright_blue(),
            usage.cache_read_tokens / 1000,
            usage.cache_write_tokens / 1000,
        )
    } else {
        String::new()
    };

    if cost_in > 0.0 {
        format!(
            "{}k in / {}k out  (~${:.4}){}",
            input_k as u32, output_k as u32, total_usd, cache_note
        )
    } else {
        format!("{}k in / {}k out{}", input_k as u32, output_k as u32, cache_note)
    }
}

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
    session_usage: Usage,
    turn_count: usize,
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
            session_usage: Usage::default(),
            turn_count: 0,
        })
    }

    fn make_spinner() -> ProgressBar {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("  {spinner:.yellow} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        pb.set_message("Thinking…");
        pb.enable_steady_tick(std::time::Duration::from_millis(80));
        pb
    }

    /// Drive the inner tool loop for a single user turn.
    async fn handle_user_turn(&mut self, input: &str) -> Result<()> {
        self.messages.push(Message::user_text(input));
        self.turn_count += 1;
        audit::record(&format!("user_turn: {}", input))?;

        for turn in 0..MAX_TURNS {
            tracing::info!(turn = turn, "calling LLM");

            let pb = Self::make_spinner();
            let pb_clone = pb.clone();
            let before_output: BeforeOutput = Box::new(move || {
                pb_clone.finish_and_clear();
            });

            let response = self
                .client
                .send(&self.system, &self.messages, &self.tool_defs, Some(before_output))
                .await
                .inspect_err(|_| pb.finish_and_clear())?;

            // Spinner cleanup for tool-only responses is handled inside send().
            pb.finish_and_clear();

            if let Some(ref u) = response.usage {
                self.session_usage.input_tokens  += u.input_tokens;
                self.session_usage.output_tokens += u.output_tokens;
                self.session_usage.cache_read_tokens  += u.cache_read_tokens;
                self.session_usage.cache_write_tokens += u.cache_write_tokens;

                let cost_str = format_cost(u, &self.model);
                println!("  {} {}", "↳".dimmed(), cost_str.dimmed());
            }

            audit::record(&format!(
                "llm_response turn={} stop_reason={}",
                turn, response.stop_reason
            ))?;

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
                let ContentBlock::ToolUse { id, name, input } = block else { continue };
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
        println!("  {}", "──────────────────────────────────────────".dimmed());
        let cmds = [
            ("/help",                   "Show this help"),
            ("/config",                 "Show provider, model, and URL"),
            ("/models",                 "List models available on the server"),
            ("/model <id>",             "Switch model for this session"),
            ("/permissions ask|auto|deny", "Change permission mode"),
            ("/clear",                  "Clear conversation history"),
            ("/compact",                "Summarize and compress conversation history"),
            ("/history",                "Show number of turns in this session"),
            ("/sessions [N]",           "Show recent sessions (default 5)"),
            ("/memory list",            "List all agent memory entries"),
            ("/memory get <key>",       "Read one memory entry"),
            ("/memory set <key> <val>", "Write a memory entry"),
            ("/memory del <key>",       "Delete a memory entry"),
            ("/audit [N]",              "Show last N audit log lines (default 20)"),
            ("/cost",                   "Show token usage and estimated cost"),
            ("/exit",                   "Quit"),
        ];
        for (cmd, desc) in cmds {
            println!("  {:<30} {}", cmd.cyan(), desc.dimmed());
        }
        println!();
    }

    fn cmd_config(&self) {
        println!();
        let provider = if self.base_url.is_some() { "openai-compatible" } else { "anthropic" };
        let url = self.base_url.as_deref().unwrap_or("https://api.anthropic.com");
        let mode = match self.permissions.mode {
            PermissionMode::Ask  => "ask",
            PermissionMode::Auto => "auto",
            PermissionMode::Deny => "deny",
        };
        println!("  {}", "Current configuration".bold());
        println!("  {}", "──────────────────────────────────────────".dimmed());
        println!("  {:<18} {}", "provider".dimmed(), provider.cyan());
        println!("  {:<18} {}", "model".dimmed(), self.model.cyan().bold());
        println!("  {:<18} {}", "base_url".dimmed(), url.cyan());
        println!("  {:<18} {}", "permissions".dimmed(), mode.cyan());
        println!("  {:<18} {}", "turns this session".dimmed(), self.turn_count.to_string().cyan());
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

    fn cmd_cost(&self) {
        println!();
        println!("  {}", "Session token usage".bold());
        println!("  {}", "──────────────────────────────────────────".dimmed());
        println!("  {:<18} {}", "input".dimmed(),
            self.session_usage.input_tokens.to_string().cyan());
        println!("  {:<18} {}", "output".dimmed(),
            self.session_usage.output_tokens.to_string().cyan());
        if self.session_usage.cache_read_tokens > 0 {
            println!("  {:<18} {}", "cache read".dimmed(),
                self.session_usage.cache_read_tokens.to_string().bright_blue());
            println!("  {:<18} {}", "cache write".dimmed(),
                self.session_usage.cache_write_tokens.to_string().bright_blue());
        }
        let (cost_in, cost_out) = cost_per_million(&self.model);
        if cost_in > 0.0 {
            let total = (self.session_usage.input_tokens as f64 * cost_in
                + self.session_usage.output_tokens as f64 * cost_out)
                / 1_000_000.0;
            println!("  {:<18} ${:.4}", "est. cost".dimmed(), total);
        }
        println!();
    }

    fn cmd_permissions(&mut self, arg: &str) {
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

    async fn cmd_compact(&mut self, config: &Config) {
        if self.messages.is_empty() {
            println!("  {} Nothing to compact.", "✗".red());
            return;
        }

        let pb = Self::make_spinner();
        pb.set_message("Compacting…");

        // Build a temporary messages list asking the model to summarize.
        let mut temp = self.messages.clone();
        temp.push(Message::user_text(
            "Please provide a concise summary of this conversation so far, \
             including the key decisions, changes made, and current state. \
             This will replace the conversation history.",
        ));

        let result = self
            .client
            .send(
                "You are a helpful assistant. Summarize the conversation concisely.",
                &temp,
                &[],
                None,
            )
            .await;

        pb.finish_and_clear();

        match result {
            Ok(resp) => {
                let summary = resp
                    .content
                    .iter()
                    .filter_map(|b| {
                        if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                let turn_count = self.messages.len();
                self.messages.clear();
                self.messages.push(Message::user_text(format!(
                    "[Conversation compacted from {} messages]\n\n{}",
                    turn_count, summary
                )));
                self.messages.push(Message {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::Text {
                        text: "Understood. I have the context from our previous conversation."
                            .to_string(),
                    }],
                });

                if let Some(u) = resp.usage {
                    self.session_usage.input_tokens  += u.input_tokens;
                    self.session_usage.output_tokens += u.output_tokens;
                }

                let _ = audit::record(&format!(
                    "compact: {} messages → 2 (summary)",
                    turn_count
                ));
                println!(
                    "  {} Compacted {} messages into a summary.",
                    "✓".green(),
                    turn_count
                );
            }
            Err(e) => println!("  {} Compact failed: {}", "✗".red(), e),
        }

        let _ = config; // used for future config-aware model selection
    }

    fn cmd_sessions(&self, arg: &str) {
        let n: usize = arg.trim().parse().unwrap_or(5).max(1).min(50);
        match persistence::init().and_then(|s| s.recent_sessions(n)) {
            Ok(rows) if rows.is_empty() => println!("  No sessions found."),
            Ok(rows) => {
                println!();
                println!("  {}", "Recent sessions".bold());
                println!("  {}", "──────────────────────────────────────────".dimmed());
                for (id, goal, model, created_at) in &rows {
                    println!(
                        "  {} {} {}  {}",
                        format!("#{}", id).dimmed(),
                        goal.cyan(),
                        format!("[{}]", model).dimmed(),
                        created_at.dimmed()
                    );
                }
                println!();
            }
            Err(e) => println!("  {} {}", "✗".red(), e),
        }
    }

    fn cmd_memory(&self, args: &str) {
        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        let subcmd = parts.first().copied().unwrap_or("list");

        let store = match persistence::init() {
            Ok(s) => s,
            Err(e) => { println!("  {} {}", "✗".red(), e); return; }
        };

        match subcmd {
            "list" | "" => {
                match store.all_memory() {
                    Ok(entries) if entries.is_empty() => println!("  No memory entries."),
                    Ok(entries) => {
                        println!();
                        println!("  {}", "Agent memory".bold());
                        println!("  {}", "──────────────────────────────────────────".dimmed());
                        for (k, v) in &entries {
                            println!("  {} = {}", k.cyan(), v);
                        }
                        println!();
                    }
                    Err(e) => println!("  {} {}", "✗".red(), e),
                }
            }
            "get" => {
                let key = parts.get(1).copied().unwrap_or("");
                if key.is_empty() {
                    println!("  Usage: /memory get <key>");
                    return;
                }
                match store.get_memory(key) {
                    Ok(Some(v)) => println!("  {} = {}", key.cyan(), v),
                    Ok(None)    => println!("  {} Key '{}' not found.", "✗".red(), key),
                    Err(e)      => println!("  {} {}", "✗".red(), e),
                }
            }
            "set" => {
                let key = parts.get(1).copied().unwrap_or("");
                let val = parts.get(2).copied().unwrap_or("");
                if key.is_empty() || val.is_empty() {
                    println!("  Usage: /memory set <key> <value>");
                    return;
                }
                match store.set_memory(key, val) {
                    Ok(_)  => println!("  {} {}", "✓".green(), format!("{} = {}", key, val).cyan()),
                    Err(e) => println!("  {} {}", "✗".red(), e),
                }
            }
            "del" | "delete" | "rm" => {
                let key = parts.get(1).copied().unwrap_or("");
                if key.is_empty() {
                    println!("  Usage: /memory del <key>");
                    return;
                }
                match store.delete_memory(key) {
                    Ok(_)  => println!("  {} Deleted '{}'.", "✓".green(), key.cyan()),
                    Err(e) => println!("  {} {}", "✗".red(), e),
                }
            }
            other => println!(
                "  {} Unknown memory subcommand '{}'. Try list/get/set/del.",
                "✗".red(), other
            ),
        }
    }

    fn cmd_audit(&self, arg: &str) {
        let n: usize = arg.trim().parse().unwrap_or(20).max(1).min(500);
        match std::fs::read_to_string(audit::AUDIT_LOG_PATH) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let start = lines.len().saturating_sub(n);
                println!();
                println!("  {} (last {} entries)", "Audit log".bold(), n);
                println!("  {}", "──────────────────────────────────────────".dimmed());
                for line in &lines[start..] {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        let ts = v["timestamp"].as_str().unwrap_or("");
                        let ev = v["event"].as_str().unwrap_or(line);
                        println!("  {} {}", ts.dimmed(), ev.cyan());
                    } else {
                        println!("  {}", line.dimmed());
                    }
                }
                println!();
            }
            Err(_) => println!("  {} No audit log found.", "✗".red()),
        }
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
                        println!("  {}", "──────────────────────────────────────────".dimmed());
                        if let Some(arr) = json["data"].as_array() {
                            for m in arr {
                                let id = m["id"].as_str().unwrap_or("?");
                                let active = if id == self.model {
                                    " ◀ active".green().to_string()
                                } else {
                                    String::new()
                                };
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
            Err(e)   => println!("  {} Could not reach server: {}", "✗".red(), e),
        }
    }

    fn cmd_model(&mut self, name: &str, config: &Config) {
        self.model = name.to_string();
        let mut new_config = config.clone();
        new_config.model = name.to_string();
        self.client = crate::llm_client::create_client(&new_config);
        println!("  {} Switched to {}", "✓".green(), name.cyan().bold());
    }

    /// Handle a `/command` line. Returns true if the session should end.
    async fn handle_slash(&mut self, line: &str, config: &Config) -> bool {
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
            "/memory"      => self.cmd_memory(arg),
            "/audit"       => self.cmd_audit(arg),
            "/compact"     => self.cmd_compact(config).await,
            "/permissions" => self.cmd_permissions(arg),
            "/model" => {
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

    // Resolve history file to $HOME/.zap_history so it persists across directories.
    let history_path = dirs::home_dir()
        .map(|h| h.join(HISTORY_PATH))
        .unwrap_or_else(|| std::path::PathBuf::from(HISTORY_PATH));

    let mut rl = Editor::<ZapHelper, DefaultHistory>::new()?;
    rl.set_helper(Some(ZapHelper));
    let _ = rl.load_history(&history_path);

    loop {
        let prompt = format!(
            "\n  {} ",
            "›".bright_yellow().bold()
        );

        match rl.readline(&prompt) {
            Ok(line) => {
                let input = line.trim().to_string();
                if input.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(&input);

                if input.starts_with('/') {
                    if session.handle_slash(&input, config).await {
                        break;
                    }
                    continue;
                }

                if input.eq_ignore_ascii_case("exit") || input.eq_ignore_ascii_case("quit") {
                    break;
                }

                if let Err(e) = session.handle_user_turn(&input).await {
                    eprintln!("  {} {}", "Error:".red().bold(), e);
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl-C — clear current line, don't exit
                println!("  {} (Ctrl+C — type /exit to quit)", "·".dimmed());
                continue;
            }
            Err(ReadlineError::Eof) => break, // Ctrl-D
            Err(e) => {
                eprintln!("  {} readline error: {}", "✗".red(), e);
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
    println!("\n  {} Goodbye.", "⚡".bright_yellow());
    audit::record("repl_end")?;
    Ok(())
}
