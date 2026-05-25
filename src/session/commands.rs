/// Slash-command handlers — all `pub fn cmd_*` / `pub async fn cmd_*` methods on Session.
/// Each handler is self-contained and can be tested independently.
use std::sync::atomic::Ordering;

use anyhow::Result;
use colored::Colorize;
use inquire::{Select, Text};

use crate::{
    audit,
    config::{Config, PermissionMode, Provider},
    llm_client::{BeforeOutput, ContentBlock, Message},
};

use super::Session;

// ── Informational ─────────────────────────────────────────────────────────────

impl Session {
    pub fn cmd_help(&self) {
        let w = 52usize;
        println!();
        println!("  {} {}  {}",
            "◆".truecolor(255, 210, 50),
            "zap".truecolor(255, 210, 50).bold(),
            "slash commands".truecolor(100, 95, 130));
        println!("  {}", "─".repeat(w).truecolor(60, 55, 80));
        let groups: &[(&str, &[(&str, &str)])] = &[
            ("session", &[
                ("/help",                    "show this help"),
                ("/config",                  "show provider, model, URL"),
                ("/cost",                    "token usage and estimated cost"),
                ("/history",                 "show message count"),
                ("/clear",                   "clear conversation history"),
                ("/compact",                 "summarize and compress history"),
                ("/sessions [N]",            "browse and resume old sessions"),
                ("/exit",                    "quit"),
            ]),
            ("model & provider", &[
                ("/model <id>",              "switch model for this session"),
                ("/models",                  "list models on server"),
                ("/provider",                "switch provider interactively"),
                ("/permissions ask|auto|deny","change permission mode"),
                ("/think [on|off|N]",        "enable extended thinking (Anthropic only)"),
                ("/goal <condition>",         "run autonomously until condition met (max 20 turns)"),
            ]),
            ("code", &[
                ("/tasks",                   "browse & execute task sessions (.zap/tasks/)"),
                ("/index [path|stats]",      "reindex AST code symbols"),
                ("/index clear",             "wipe index and rebuild from scratch"),
                ("/index quality",           "code quality report: god objects, coupling, dead code"),
                ("/undo [file]",             "undo last file edit"),
                ("/init",                    "set up this project (ZAP.md, index, project.json)"),
                ("/run <workflow>",          "run a workflow from .zap/workflows/"),
                ("/deploy [--check]",        "build & install zap (live output, no timeout)"),
            ]),
            ("media", &[
                ("/attach <path>",           "stage an image for next message"),
                ("/paste",                   "paste image from clipboard"),
            ]),
            ("memory & skills", &[
                ("/memory list",             "list memory entries"),
                ("/memory get <key>",        "read a memory entry"),
                ("/memory set <k> <v>",      "write a memory entry"),
                ("/memory del <key>",        "delete a memory entry"),
                ("/skill list",               "list available skills"),
                ("/skill log",               "show which skills fired (or didn't) per turn"),
                ("/skill scope",              "show/change domain skill scope for this session"),
                ("/skill show <name>",        "preview a skill"),
                ("/skill export <name>",      "copy a built-in skill to ~/.zap/skills/ for editing"),
                ("/skill export --all",       "export all built-in skills to ~/.zap/skills/"),
                ("/skill create <name>",      "create a new skill file"),
                ("/audit [N]",               "show last N audit log lines"),
                ("/hooks",                   "list configured hooks"),
                ("/mcp [list|edit|path]",    "view/edit MCP server configs"),
            ]),
        ];
        for (group, cmds) in groups {
            println!();
            println!("  {} {}", "▸".truecolor(255, 210, 50), group.truecolor(150, 140, 170).bold());
            for (cmd, desc) in *cmds {
                println!("    {:<32} {}",
                    cmd.truecolor(100, 210, 255),
                    desc.truecolor(100, 95, 130));
            }
        }
        println!();
        println!("  {}", "─".repeat(w).truecolor(60, 55, 80));
        println!("  {} {}", "tip:".truecolor(100, 95, 130),
            "press / on empty input to open the command picker".truecolor(100, 95, 130));
        println!();
    }

    pub fn cmd_config(&self) {
        let provider_label = match self.config.provider {
            Provider::Anthropic => "Anthropic API",
            Provider::OpenAi    => "OpenAI-compatible",
        };
        // Strip the endpoint suffix for cleaner display.
        let url_raw  = self.base_url.as_deref().unwrap_or("https://api.anthropic.com/v1/messages");
        let url_raw  = url_raw.strip_suffix("/chat/completions").unwrap_or(url_raw);
        let url      = url_raw.strip_suffix("/v1").unwrap_or(url_raw);
        let mode = match self.permissions.mode {
            PermissionMode::Ask  => "ask",
            PermissionMode::Auto => "auto",
            PermissionMode::Deny => "deny",
        };
        let depth_label = if self.config.agent_depth > 0 {
            format!("enabled (depth {})", self.config.agent_depth)
        } else {
            "disabled".to_string()
        };

        println!();
        println!("  {} {}", "◆".truecolor(255, 210, 50), "configuration".truecolor(150, 140, 170).bold());
        println!("  {}", "─".repeat(44).truecolor(60, 55, 80));
        let kv = |k: &str, v: &str| {
            println!("  {:<20} {}", k.truecolor(100, 95, 130), v.truecolor(100, 210, 255).bold());
        };
        kv("provider",           provider_label);
        kv("model",              &self.model);
        kv("base_url",           url);
        kv("permissions",        mode);
        kv("sub-agents",         &depth_label);
        kv("turns this session", &self.turn_count.to_string());
        // Network / proxy settings
        if let Some(ref p) = self.config.proxy {
            kv("proxy",          p);
        }
        if let Some(ref ca) = self.config.ca_bundle {
            kv("ca_bundle",      ca);
        }
        if self.config.tls_skip_verify {
            kv("tls_verify",     "DISABLED");
        }
        if self.config.timeout_secs != 120 {
            kv("timeout",        &format!("{}s", self.config.timeout_secs));
        }
        kv("log file",           &crate::log::log_path().to_string_lossy());
        kv("llm log",            &crate::log::llm_log_path().to_string_lossy());
        if !self.config.skill_paths.is_empty() {
            kv("skill_paths", &self.config.skill_paths.join(", "));
        }
        println!("  {}", "─".repeat(44).truecolor(60, 55, 80));
        println!();
    }

    pub fn cmd_history(&self) {
        println!("  {} messages in history", self.messages.len().to_string().cyan());
    }

    pub fn cmd_cost(&self) {
        use crate::ui::cost_per_million;
        println!();
        println!("  {}", "Session token usage".bold());
        println!("  {}", "──────────────────────────────────────────".dimmed());
        println!("  {:<18} {}", "input".dimmed(),  self.session_usage.input_tokens.to_string().cyan());
        println!("  {:<18} {}", "output".dimmed(), self.session_usage.output_tokens.to_string().cyan());
        if self.session_usage.cache_read_tokens > 0 {
            println!("  {:<18} {}", "cache read".dimmed(),
                self.session_usage.cache_read_tokens.to_string().bright_blue());
            println!("  {:<18} {}", "cache write".dimmed(),
                self.session_usage.cache_write_tokens.to_string().bright_blue());
        }
        let (cost_in, cost_out) = cost_per_million(&self.model);
        if cost_in > 0.0 {
            let total = (self.session_usage.input_tokens  as f64 * cost_in
                       + self.session_usage.output_tokens as f64 * cost_out)
                       / 1_000_000.0;
            println!("  {:<18} ${:.4}", "est. cost".dimmed(), total);
        }
        println!();
    }

    pub fn cmd_audit(&self, arg: &str) {
        let n: usize = arg.trim().parse().unwrap_or(20).max(1).min(500);
        match std::fs::read_to_string(audit::audit_log_path()) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let start = lines.len().saturating_sub(n);
                println!();
                println!("  {} (last {} entries)", "Audit log".bold(), n);
                println!("  {}", "──────────────────────────────────────────".dimmed());
                for line in &lines[start..] {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        println!("  {} {}", v["timestamp"].as_str().unwrap_or("").dimmed(),
                            v["event"].as_str().unwrap_or(line).cyan());
                    } else {
                        println!("  {}", line.dimmed());
                    }
                }
                println!();
            }
            Err(_) => println!("  {} No audit log found.", "✗".red()),
        }
    }
}

// ── Session state ─────────────────────────────────────────────────────────────

impl Session {
    pub fn cmd_clear(&mut self) {
        self.messages.clear();
        println!("  {} History cleared.", "✓".green());
    }

    pub fn cmd_permissions(&mut self, arg: &str) {
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

    pub fn cmd_model(&mut self, name: &str, config: &Config) {
        self.model = name.to_string();
        let mut new_config = config.clone();
        new_config.model   = name.to_string();
        self.client = crate::llm_client::create_client(&new_config);
        println!("  {} Switched to {}", "✓".green(), name.cyan().bold());
    }

    /// Summarise conversation history in-place. Returns true on success.
    /// Tracks failures in `self.compact_failures`; resets to 0 on success.
    pub async fn cmd_compact(&mut self) -> bool {
        if self.messages.is_empty() {
            println!("  {} Nothing to compact.", "✗".red());
            return false;
        }
        let mut spinner = Self::make_spinner();
        let mut temp = self.messages.clone();
        temp.push(Message::user_text(
            "Please provide a concise summary of this conversation so far. \
             Include: the original task or goal, key decisions made, files created or \
             modified, errors encountered and how they were resolved, and the current \
             state. Preserve any explicit user instructions or preferences. \
             This summary will replace the full conversation history.",
        ));

        let result = self.client.send(
            "You are a helpful assistant. Summarize the conversation concisely, \
             preserving all task-critical context and user instructions.",
            &temp, &[], None, 0,
        ).await;
        spinner.finish_and_clear();

        match result {
            Ok(resp) => {
                let summary = resp.content.iter()
                    .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                    .collect::<Vec<_>>().join("\n");

                let turn_count = self.messages.len();
                self.messages.clear();
                self.messages.push(Message::user_text(format!(
                    "[Conversation compacted from {} messages]\n\n{}", turn_count, summary
                )));
                self.messages.push(Message {
                    role:    "assistant".to_string(),
                    content: vec![ContentBlock::Text {
                        text: "Understood. I have the context from our previous conversation.".to_string(),
                    }],
                });
                if let Some(u) = resp.usage {
                    self.session_usage.input_tokens  += u.input_tokens;
                    self.session_usage.output_tokens += u.output_tokens;
                }
                let _ = audit::record(&format!(
                    "compact: {} messages → 2 (summary) model={}", turn_count, self.config.model
                ));
                self.compact_failures = 0;
                println!("  {} Compacted {} messages into a summary.", "✓".green(), turn_count);
                true
            }
            Err(e) => {
                self.compact_failures += 1;
                println!("  {} Compact failed: {}", "✗".red(), e);
                if self.compact_failures >= 3 {
                    println!(
                        "  {} Compact failed 3 times — use {} to reset or {} to start fresh.",
                        "⚠".bright_yellow(), "/clear".cyan(), "/exit".cyan()
                    );
                }
                false
            }
        }
    }
}

// ── Sessions ──────────────────────────────────────────────────────────────────

/// Truncate a string to at most `max` characters, adding "…" if shortened.
/// Uses char-aware slicing to avoid panicking on multi-byte boundaries.
fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn print_conversation(messages: &[Message]) {
    if messages.is_empty() { return; }
    println!();
    println!("  {}", "── Conversation ──".bold());
    for msg in messages {
        let role_label = match msg.role.as_str() {
            "user" => format!("  {} ", "[You]".green().bold()),
            "assistant" => format!("  {} ", "[Zap]".cyan().bold()),
            _ => format!("  {} ", format!("[{}]", msg.role).dimmed()),
        };
        let mut first = true;
        for block in &msg.content {
            match block {
                ContentBlock::Text { text } => {
                    let prefix = format!("  {} ", "│".dimmed());
                    for line in text.lines() {
                        if first {
                            // First line gets the role label as prefix
                            println!("{}{}", role_label, line.dimmed());
                            first = false;
                        } else if line.is_empty() {
                            println!("{}", prefix);
                        } else {
                            println!("{}{}", prefix, line.dimmed());
                        }
                    }
                }
                ContentBlock::ToolUse { name, input, .. } => {
                    let input_str = serde_json::to_string(input)
                        .unwrap_or_else(|_| "[serialization error]".to_string());
                    let preview = truncate_preview(&input_str, 77);
                    println!("  {} {} {}", "▸".yellow(), name.yellow(), preview.dimmed());
                    first = false;
                }
                ContentBlock::ToolResult { content, .. } => {
                    let preview = truncate_preview(content, 117);
                    println!("  {} {}", "◂".blue(), preview.dimmed());
                    first = false;
                }
                ContentBlock::Image { media_type, .. } => {
                    println!("  {} [image: {}]", "🖼".dimmed(), media_type.dimmed());
                    first = false;
                }
                ContentBlock::Thinking { thinking, .. } => {
                    let preview = truncate_preview(thinking, 77);
                    println!("  {} {}", "💭".dimmed(), preview.dimmed());
                    first = false;
                }
                ContentBlock::Reasoning { content } => {
                    let preview = truncate_preview(content, 77);
                    println!("  {} {}", "🧠".dimmed(), preview.dimmed());
                    first = false;
                }
            }
        }
    }
    println!("  {}", "──────────────────".dimmed());
    println!();
}

impl Session {
    pub fn cmd_sessions(&mut self, _arg: &str) {
        let rows = match self.store.recent_sessions(20) {
            Ok(r) if r.is_empty() => { println!("  No sessions found."); return; }
            Ok(r) => r,
            Err(e) => { println!("  {} {}", "✗".red(), e); return; }
        };

        let cfg = crate::ui::inquire_render_config();
        let entries: Vec<String> = rows.iter().map(|(id, goal, model, ts)| {
            let date       = ts.get(..10).unwrap_or(ts.as_str());
            let goal_short = if goal.len() > 50 { format!("{}…", &goal[..49]) } else { goal.clone() };
            format!("#{:<4} {:<52} [{:<20}] {}", id, goal_short, model, date)
        }).collect();

        let result = Select::new("Resume session:", entries.clone())
            .with_render_config(cfg)
            .with_help_message("↑↓ navigate   type to filter   Enter load   Esc cancel")
            .with_page_size(12)
            .prompt_skippable();

        let chosen_idx = match result {
            Ok(Some(line)) => entries.iter().position(|e| e == &line),
            _ => None,
        };

        if let Some(idx) = chosen_idx {
            let (session_id, goal, model, _) = &rows[idx];
            // Always show session content (goal + files).
            let files_info = crate::project::session_log_files(*session_id)
                .map(|f| format!("  {} Files: {}", "◌".dimmed(), f.dimmed()))
                .unwrap_or_default();
            match self.store.load_messages(*session_id) {
                Ok(Some(json)) => match serde_json::from_str::<Vec<Message>>(&json) {
                    Ok(msgs) => {
                        self.messages   = msgs;
                        self.turn_count = self.messages.iter().filter(|m| m.role == "user").count();
                        self.session_id = *session_id;
                        self.model      = model.clone();
                        let mut cfg     = self.config.clone();
                        cfg.model       = model.clone();
                        self.client     = crate::llm_client::create_client(&cfg);
                        println!("  {} Loaded session #{} — {} messages, model {}",
                            "✓".green(), session_id, self.messages.len().to_string().cyan(), model.cyan());
                        println!("  {} {}", "◌".dimmed(), goal.dimmed());
                        if !files_info.is_empty() {
                            println!("{}", files_info);
                        }
                        print_conversation(&self.messages);
                    }
                    Err(e) => println!("  {} Could not parse messages: {}", "✗".red(), e),
                },
                Ok(None) => {
                    println!("  {} Session #{} has no saved messages.", "◌".yellow(), session_id);
                    println!("  {} {}", "◌".dimmed(), goal.dimmed());
                    if !files_info.is_empty() {
                        println!("{}", files_info);
                    }
                },
                Err(e)   => println!("  {} {}", "✗".red(), e),
            }
        }
    }
}

// ── Provider / model ──────────────────────────────────────────────────────────

impl Session {
    pub fn cmd_provider(&mut self, config: &Config) {
        #[derive(Clone)]
        struct ProviderDef {
            slug:        &'static str,
            name:        &'static str,
            hint:        &'static str,
            kind:        ProviderKind,
            models:      &'static [&'static str],
            /// Full endpoint URL stored in TOML, e.g. ".../v1/chat/completions".
            /// None = provider uses the client's built-in default URL.
            base_url:    Option<&'static str>,
            needs_key:   bool,
            coming_soon: bool,
        }
        #[derive(Clone)]
        enum ProviderKind { Anthropic, OpenAi }

        let providers: Vec<ProviderDef> = vec![
            ProviderDef { slug: "lm_studio",  name: "LM Studio",                  hint: "local · OpenAI-compatible",                    kind: ProviderKind::OpenAi,    models: &["gemma-4-e4b-it", "qwen2.5-coder-7b-instruct", "mistral-7b-instruct", "Other…"],    base_url: Some("http://localhost:1234/v1/chat/completions"),                                    needs_key: false, coming_soon: false },
            ProviderDef { slug: "ollama",     name: "Ollama",                     hint: "local · OpenAI-compatible",                    kind: ProviderKind::OpenAi,    models: &["llama3.2", "llama3.1:70b", "codellama", "qwen2.5-coder", "Other…"],                 base_url: Some("http://localhost:11434/v1/chat/completions"),                                   needs_key: false, coming_soon: false },
            ProviderDef { slug: "anthropic",  name: "Anthropic",                  hint: "claude-sonnet-4-6 / claude-opus-4-7",          kind: ProviderKind::Anthropic, models: &["claude-sonnet-4-6", "claude-opus-4-7", "claude-haiku-4-5", "Other…"],               base_url: None,                                                                                needs_key: true,  coming_soon: false },
            ProviderDef { slug: "claude_code",name: "Claude Code (Pro/Max API)",  hint: "full API via subscription · after 16 Jun 2026", kind: ProviderKind::Anthropic, models: &["claude-sonnet-4-6", "claude-opus-4-7"],                                             base_url: None,                                                                                needs_key: false, coming_soon: true  },
            ProviderDef { slug: "openai",     name: "OpenAI",                     hint: "gpt-4o / gpt-4o-mini / o3",                    kind: ProviderKind::OpenAi,    models: &["gpt-4o", "gpt-4o-mini", "o3", "o4-mini", "Other…"],                                 base_url: None,                                                                                needs_key: true,  coming_soon: false },
            ProviderDef { slug: "gemini",     name: "Google Gemini",              hint: "gemini-2.5-pro / gemini-2.0-flash",            kind: ProviderKind::OpenAi,    models: &["gemini-2.0-flash", "gemini-2.5-pro", "gemini-2.5-flash", "Other…"],                 base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"),    needs_key: true,  coming_soon: false },
            ProviderDef { slug: "deepseek",   name: "DeepSeek",                   hint: "deepseek-v4-pro / deepseek-v4-flash",         kind: ProviderKind::OpenAi,    models: &["deepseek-v4-pro", "deepseek-v4-flash", "deepseek-chat", "deepseek-reasoner", "Other…"], base_url: Some("https://api.deepseek.com/v1/chat/completions"),                           needs_key: true,  coming_soon: false },
            ProviderDef { slug: "groq",       name: "Groq",                       hint: "llama-3.3-70b · fastest inference",            kind: ProviderKind::OpenAi,    models: &["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "mixtral-8x7b-32768", "Other…"], base_url: Some("https://api.groq.com/openai/v1/chat/completions"),                             needs_key: true,  coming_soon: false },
            ProviderDef { slug: "mistral",    name: "Mistral",                    hint: "mistral-large / codestral",                    kind: ProviderKind::OpenAi,    models: &["mistral-large-latest", "codestral-latest", "mistral-small-latest", "Other…"],       base_url: Some("https://api.mistral.ai/v1/chat/completions"),                                  needs_key: true,  coming_soon: false },
            ProviderDef { slug: "xai",        name: "xAI (Grok)",                 hint: "grok-3 / grok-3-mini",                         kind: ProviderKind::OpenAi,    models: &["grok-3", "grok-3-mini", "grok-2", "Other…"],                                       base_url: Some("https://api.x.ai/v1/chat/completions"),                                        needs_key: true,  coming_soon: false },
            ProviderDef { slug: "together",   name: "Together AI",                hint: "Llama / Qwen / Mistral open models",           kind: ProviderKind::OpenAi,    models: &["meta-llama/Llama-3-70b-chat-hf", "Qwen/Qwen2.5-72B-Instruct-Turbo", "Other…"],    base_url: Some("https://api.together.xyz/v1/chat/completions"),                                needs_key: true,  coming_soon: false },
            ProviderDef { slug: "perplexity", name: "Perplexity",                 hint: "sonar-pro · web-grounded answers",             kind: ProviderKind::OpenAi,    models: &["sonar-pro", "sonar", "sonar-reasoning", "Other…"],                                  base_url: Some("https://api.perplexity.ai/chat/completions"),                                  needs_key: true,  coming_soon: false },
            ProviderDef { slug: "cohere",     name: "Cohere",                     hint: "command-r-plus",                               kind: ProviderKind::OpenAi,    models: &["command-r-plus", "command-r", "Other…"],                                            base_url: Some("https://api.cohere.ai/compatibility/v1/chat/completions"),                    needs_key: true,  coming_soon: false },
            ProviderDef { slug: "custom",     name: "Custom (OpenAI-compatible)", hint: "any OpenAI-compatible endpoint",               kind: ProviderKind::OpenAi,    models: &["Other…"],                                                                           base_url: None,                                                                                needs_key: false, coming_soon: false },
        ];

        let labels: Vec<String> = providers.iter().map(|p| {
            if p.coming_soon { format!("{:<26}· {}  ◷ coming 16 Jun 2026", p.name, p.hint) }
            else             { format!("{:<26}· {}", p.name, p.hint) }
        }).collect();

        let cfg = crate::ui::inquire_render_config();

        let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let chosen = match Select::new("Switch provider:", label_refs)
            .with_render_config(cfg.clone())
            .with_help_message("↑↓ navigate   Enter select   Esc cancel")
            .with_page_size(14)
            .prompt_skippable()
        {
            Ok(Some(s)) => s.to_string(),
            _ => return,
        };

        let idx = labels.iter().position(|l| l == &chosen).unwrap_or(0);
        let def = &providers[idx];

        if def.coming_soon {
            println!();
            println!("  {} {}", "◷".truecolor(255, 210, 50), "Claude Code (Pro/Max API)".truecolor(255, 210, 50).bold());
            println!("  {}", "─".repeat(52).truecolor(60, 55, 80));
            println!("  Anthropic is adding Agent SDK credits to Pro/Max plans");
            println!("  on {} — enabling direct API access without an API key.", "16 Jun 2026".truecolor(100, 210, 255).bold());
            println!();
            println!("  {} Use {} today for Pro/Max access with an API key.",
                "tip:".truecolor(100, 95, 130), "Anthropic".truecolor(100, 210, 255));
            println!();
            return;
        }

        let base_url = if def.slug == "custom" {
            match Text::new("Full endpoint URL (e.g. http://localhost:8080/v1/chat/completions):")
                .prompt_skippable()
            {
                Ok(Some(u)) if !u.trim().is_empty() => Some(u.trim().to_string()),
                _ => { println!("  Cancelled."); return; }
            }
        } else {
            def.base_url.map(str::to_string)
        };

        // Retrieve any existing entry for this provider so we can preserve the key.
        let existing_entry = config.all_providers.get(def.slug);

        let api_key = if def.needs_key {
            let existing_key = existing_entry
                .and_then(|e| e.api_key.as_deref())
                .filter(|k| !k.is_empty())
                .unwrap_or("");
            let prompt = if existing_key.is_empty() {
                "API key:".to_string()
            } else {
                format!("API key (blank = keep existing {}…{}):", &existing_key[..4.min(existing_key.len())], &existing_key[existing_key.len().saturating_sub(4)..])
            };
            match Text::new(&prompt)
                .with_render_config(cfg.clone())
                .with_help_message("Saved to ~/.agent.toml")
                .prompt_skippable()
            {
                Ok(Some(k)) if !k.trim().is_empty() => k.trim().to_string(),
                _ => existing_key.to_string(),
            }
        } else {
            String::new()
        };

        let model_input = {
            match Select::new("Model:", def.models.to_vec())
                .with_render_config(cfg.clone())
                .with_help_message("↑↓ navigate   Enter select   Esc = keep current")
                .with_page_size(10)
                .prompt_skippable()
            {
                Ok(Some(m)) if m == "Other…" => {
                    match Text::new("Enter model name:").with_render_config(cfg).prompt_skippable() {
                        Ok(Some(m)) if !m.trim().is_empty() => m.trim().to_string(),
                        _ => def.models[0].to_string(),
                    }
                }
                Ok(Some(m)) => m.to_string(),
                _ => def.models[0].to_string(),
            }
        };

        let kind_str = match def.kind {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenAi    => "openai",
        };

        let mut new_config      = config.clone();
        new_config.provider     = match def.kind { ProviderKind::Anthropic => Provider::Anthropic, ProviderKind::OpenAi => Provider::OpenAi };
        new_config.provider_slug = def.slug.to_string();
        new_config.model        = model_input.clone();
        new_config.base_url     = base_url.clone();
        new_config.api_key      = api_key.clone();

        // Update (or insert) this provider's entry while preserving all others.
        new_config.all_providers.insert(def.slug.to_string(), crate::config::ProviderEntry {
            kind:     Some(kind_str.to_string()),
            model:    Some(model_input.clone()),
            api_key:  if api_key.is_empty() { None } else { Some(api_key) },
            base_url: base_url.clone(),
        });

        self.client   = crate::llm_client::create_client(&new_config);
        self.model    = model_input.clone();
        self.base_url = new_config.base_url.clone();
        self.config   = new_config.clone();

        match new_config.save() {
            Ok(_)  => println!("  {} Switched to {} · {}  {}", "✓".green(), def.name.cyan().bold(), model_input.cyan(), "(saved to ~/.agent.toml)".dimmed()),
            Err(e) => println!("  {} Switched to {} · {}  {} {}", "✓".green(), def.name.cyan().bold(), model_input.cyan(), "warn: could not save:".yellow(), e),
        }
    }

    pub async fn cmd_models(&self) {
        let url = match &self.base_url {
            Some(b) => {
                // base_url may be a full endpoint (e.g. ".../v1/chat/completions").
                // Strip the chat/completions suffix to recover the v1 base.
                let b = b.trim_end_matches('/');
                let base = b.strip_suffix("/chat/completions").unwrap_or(b);
                format!("{}/models", base.trim_end_matches('/'))
            }
            None => {
                println!("  {} /models only works with OpenAI-compatible servers.", "✗".red());
                return;
            }
        };
        let client = crate::http::client();
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => {
                        println!();
                        println!("  {}", "Available models".bold());
                        println!("  {}", "──────────────────────────────────────────".dimmed());
                        if let Some(arr) = json["data"].as_array() {
                            for m in arr {
                                let id     = m["id"].as_str().unwrap_or("?");
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
            Err(e)   => println!("  {} Could not reach server: {}", "✗".red(), e),
        }
    }
}

// ── Memory ────────────────────────────────────────────────────────────────────

impl Session {
    pub fn cmd_memory(&self, args: &str) {
        let parts: Vec<&str> = args.splitn(3, ' ').collect();
        let subcmd = parts.first().copied().unwrap_or("list");

        match subcmd {
            "list" | "" => match self.store.all_memory() {
                Ok(entries) if entries.is_empty() => println!("  No memory entries."),
                Ok(entries) => {
                    println!();
                    println!("  {}", "Agent memory".bold());
                    println!("  {}", "──────────────────────────────────────────".dimmed());
                    for (k, v) in &entries { println!("  {} = {}", k.cyan(), v); }
                    println!();
                }
                Err(e) => println!("  {} {}", "✗".red(), e),
            },
            "get" => {
                let key = parts.get(1).copied().unwrap_or("");
                if key.is_empty() { println!("  Usage: /memory get <key>"); return; }
                match self.store.get_memory(key) {
                    Ok(Some(v)) => println!("  {} = {}", key.cyan(), v),
                    Ok(None)    => println!("  {} Key '{}' not found.", "✗".red(), key),
                    Err(e)      => println!("  {} {}", "✗".red(), e),
                }
            }
            "set" => {
                let key = parts.get(1).copied().unwrap_or("");
                let val = parts.get(2).copied().unwrap_or("");
                if key.is_empty() || val.is_empty() { println!("  Usage: /memory set <key> <value>"); return; }
                match self.store.set_memory(key, val) {
                    Ok(_)  => println!("  {} {}", "✓".green(), format!("{} = {}", key, val).cyan()),
                    Err(e) => println!("  {} {}", "✗".red(), e),
                }
            }
            "del" | "delete" | "rm" => {
                let key = parts.get(1).copied().unwrap_or("");
                if key.is_empty() { println!("  Usage: /memory del <key>"); return; }
                match self.store.delete_memory(key) {
                    Ok(_)  => println!("  {} Deleted '{}'.", "✓".green(), key.cyan()),
                    Err(e) => println!("  {} {}", "✗".red(), e),
                }
            }
            other => println!("  {} Unknown memory subcommand '{}'. Try list/get/set/del.", "✗".red(), other),
        }
    }
}

// ── MCP ───────────────────────────────────────────────────────────────────────

impl Session {
    pub fn cmd_mcp(&self, arg: &str) {
        let global_path = dirs::home_dir()
            .map(|h| h.join(".zap").join("mcp.json"));
        let project_path = std::path::PathBuf::from(".mcp.json");

        match arg.trim() {
            // ── list (default) ────────────────────────────────────────────
            "" | "list" => {
                // Read both files independently so we can show origin.
                let global_cfg = global_path.as_ref()
                    .filter(|p| p.exists())
                    .map(|p| crate::mcp::load_file(p))
                    .unwrap_or_default();

                let project_cfg = if project_path.exists() {
                    crate::mcp::load_file(&project_path)
                } else {
                    crate::mcp::McpConfig::default()
                };

                // Collect pending names from the tool registry.
                let pending: std::collections::HashSet<String> = self.tools
                    .pending_mcp_servers()
                    .into_iter()
                    .map(|(n, _)| n.to_string())
                    .collect();

                let print_servers = |servers: &std::collections::HashMap<String, crate::mcp::McpServerConfig>| {
                    if servers.is_empty() {
                        println!("    {}", "(none)".truecolor(100, 95, 130));
                        return;
                    }
                    for (name, cfg) in servers {
                        let status = if pending.contains(name) {
                            "pending".truecolor(180, 130, 60)
                        } else {
                            "connected".truecolor(100, 200, 100)
                        };
                        println!("    {} {} {}  {}",
                            "◆".truecolor(255, 210, 50),
                            name.truecolor(100, 210, 255).bold(),
                            format!("[{}]", status),
                            cfg.command.truecolor(100, 95, 130),
                        );
                        if let Some(ref desc) = cfg.description {
                            println!("      {}", desc.truecolor(120, 115, 140));
                        }
                    }
                };

                println!();
                println!("  {} {}", "◆".truecolor(255, 210, 50), "MCP servers".truecolor(150, 140, 170).bold());
                println!("  {}", "─".repeat(44).truecolor(60, 55, 80));

                let gpath_str = global_path.as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "~/.zap/mcp.json".to_string());
                println!("  {} {}", "global".truecolor(100, 95, 130), gpath_str.truecolor(60, 55, 80));
                print_servers(&global_cfg.servers);

                println!("  {} {}", "project".truecolor(100, 95, 130), project_path.display().to_string().truecolor(60, 55, 80));
                print_servers(&project_cfg.servers);

                println!("  {}", "─".repeat(44).truecolor(60, 55, 80));
                println!("  {} {} · {} · {}",
                    "tip:".truecolor(100, 95, 130),
                    "/mcp edit".truecolor(100, 210, 255),
                    "/mcp edit project".truecolor(100, 210, 255),
                    "/mcp path".truecolor(100, 210, 255),
                );
                println!();
            }

            // ── edit global ───────────────────────────────────────────────
            "edit" | "edit global" => {
                let path = match global_path {
                    Some(ref p) => p.clone(),
                    None => { println!("  {} could not determine home dir", "✗".red()); return; }
                };
                // Ensure parent exists and seed an empty config if missing.
                if !path.exists() {
                    std::fs::create_dir_all(path.parent().unwrap()).ok();
                    std::fs::write(&path,
                        "{\n  \"mcpServers\": {\n  }\n}\n"
                    ).ok();
                    println!("  {} created {}", "✓".green(), path.display().to_string().cyan());
                }
                open_in_editor(&path);
            }

            // ── edit project ──────────────────────────────────────────────
            "edit project" => {
                if !project_path.exists() {
                    std::fs::write(&project_path,
                        "{\n  \"mcpServers\": {\n  }\n}\n"
                    ).ok();
                    println!("  {} created {}", "✓".green(), project_path.display().to_string().cyan());
                }
                open_in_editor(&project_path);
            }

            // ── path: just print the paths ────────────────────────────────
            "path" | "paths" => {
                if let Some(ref p) = global_path {
                    println!("  {} {}", "global: ".truecolor(100, 95, 130), p.display().to_string().cyan());
                }
                println!("  {} {}", "project:".truecolor(100, 95, 130), project_path.display().to_string().cyan());
            }

            other => {
                println!("  {} unknown subcommand '{}'. Try: list · edit · edit project · path",
                    "✗".red(), other);
            }
        }
    }
}

fn open_in_editor(path: &std::path::Path) {
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| "vi".to_string());
    match std::process::Command::new(&editor).arg(path).status() {
        Ok(s) if s.success() => {}
        Ok(_) => {}
        Err(e) => {
            println!("  {} could not open editor '{}': {}", "✗".red(), editor, e);
            println!("  Edit manually: {}", path.display().to_string().cyan());
        }
    }
}

// ── Media ─────────────────────────────────────────────────────────────────────

impl Session {
    pub fn cmd_paste(&mut self) {
        if !crate::llm_client::provider_supports_vision(&self.config) {
            println!("  {} {} does not support vision — image will not be sent.",
                "✗".red(),
                self.config.base_url.as_deref().unwrap_or("this provider").cyan());
            println!("  {} Switch to Claude or GPT-4o to use images.", "·".dimmed());
            return;
        }

        #[cfg(windows)]
        let tmp = r"C:\Windows\Temp\zap_clipboard_paste.png";
        #[cfg(not(windows))]
        let tmp = "/tmp/zap_clipboard_paste.png";

        let ok = paste_clipboard_image(tmp);

        if ok && std::path::Path::new(tmp).exists() {
            self.cmd_attach(tmp);
        } else {
            println!("  {} No image in clipboard. Copy a screenshot first, then run /paste.", "✗".red());
            println!("  {} You can also use {} to stage a file directly.", "·".dimmed(), "/attach <path>".cyan());
        }
    }

    pub fn cmd_attach(&mut self, path: &str) {
        if !crate::llm_client::provider_supports_vision(&self.config) {
            println!("  {} {} does not support vision — image will not be sent.",
                "✗".red(),
                self.config.base_url.as_deref().unwrap_or("this provider").cyan());
            println!("  {} Switch to Claude or GPT-4o to use images.", "·".dimmed());
            return;
        }

        let path = path.trim();
        if path.is_empty() {
            println!("  Usage: /attach <image-path>");
            return;
        }
        let mime = match std::path::Path::new(path)
            .extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref()
        {
            Some("png")            => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif")            => "image/gif",
            Some("webp")           => "image/webp",
            _ => {
                println!("  {} Unsupported format. Use png / jpg / gif / webp.", "✗".red());
                return;
            }
        };
        match std::fs::read(path) {
            Ok(bytes) => {
                use base64::Engine;
                let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                let kb   = bytes.len() / 1024;
                println!("  {} Attached {} ({} KB, {})", "✓".green(), path.cyan(), kb, mime.dimmed());
                self.staged_images.push((mime.to_string(), data));
            }
            Err(e) => println!("  {} Could not read '{}': {}", "✗".red(), path, e),
        }
    }
}

/// Try every available method to write the clipboard image to `dest`.
/// Returns true if the file was written successfully.
pub fn paste_clipboard_image(dest: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        // Fast path: pngpaste CLI (brew install pngpaste)
        if std::process::Command::new("pngpaste")
            .arg(dest)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return true;
        }
        // Fallback: AppleScript
        let script = format!(
            r#"try
  set d to (the clipboard as «class PNGf»)
  set f to open for access POSIX file "{dest}" with write permission
  set eof f to 0
  write d to f
  close access f
  return true
on error
  return false
end try"#
        );
        return std::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
            .unwrap_or(false);
    }

    #[cfg(target_os = "windows")]
    {
        // PowerShell: System.Windows.Forms has Clipboard::GetImage()
        let script = format!(
            r#"Add-Type -Assembly System.Windows.Forms; \
$img = [System.Windows.Forms.Clipboard]::GetImage(); \
if ($img -eq $null) {{ exit 1 }}; \
$img.Save('{dest}'); exit 0"#
        );
        return std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Linux: try xclip then wl-paste (Wayland)
        let xclip_ok = std::process::Command::new("xclip")
            .args(["-selection", "clipboard", "-t", "image/png", "-o"])
            .output()
            .map(|o| {
                if o.status.success() && !o.stdout.is_empty() {
                    std::fs::write(dest, &o.stdout).is_ok()
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if xclip_ok { return true; }

        std::process::Command::new("wl-paste")
            .args(["--type", "image/png"])
            .output()
            .map(|o| {
                if o.status.success() && !o.stdout.is_empty() {
                    std::fs::write(dest, &o.stdout).is_ok()
                } else {
                    false
                }
            })
            .unwrap_or(false)
    }
}

// ── Code ──────────────────────────────────────────────────────────────────────

impl Session {
    pub fn cmd_init(&mut self) -> Option<String> {
        let project_type = detect_project_type();
        let cfg = crate::ui::inquire_render_config();

        // ── 1. Confirm / correct detected language ────────────────────────────
        println!(
            "  {} Detected project type: {}",
            "◌".dimmed(),
            project_type.cyan(),
        );
        let language_input = inquire::Text::new("Language(s) for this project:")
            .with_initial_value(project_type)
            .with_render_config(cfg)
            .prompt()
            .unwrap_or_else(|_| project_type.to_string());
        let languages: Vec<String> = language_input
            .split([',', ' '])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect();

        // ── 2. Offer indexing ─────────────────────────────────────────────────
        println!(
            "  {} Indexing lets zap find symbols and definitions instantly without reading every file.",
            "◌".dimmed(),
        );
        let do_index = inquire::Confirm::new("Index this project now? (recommended, ~10s)")
            .with_default(true)
            .prompt()
            .unwrap_or(true);
        if do_index {
            self.cmd_index("");
            crate::project::mark_indexed();
        }

        // ── 3. Write / update project.json ────────────────────────────────────
        let cwd_name = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "project".to_string());
        let meta = crate::project::ProjectMeta {
            name: cwd_name,
            language: languages,
            indexed: do_index,
            indexed_at: if do_index { Some(chrono::Utc::now().to_rfc3339()) } else { None },
            initialized_at: Some(chrono::Utc::now().to_rfc3339()),
        };
        if let Err(e) = crate::project::save_project_meta(&meta) {
            println!("  {} Could not write project.json: {}", "✗".red(), e);
        } else {
            println!("  {} .zap/project.json written.", "✓".green());
        }

        // ── 4. Create ZAP.md if it doesn't exist ─────────────────────────────
        let zap_md = std::path::Path::new("ZAP.md");
        if zap_md.exists() {
            println!("  {} ZAP.md already exists — skipping template.", "◌".dimmed());
            println!(
                "  {} Project initialized. zap will remember this project.",
                "✓".green(),
            );
            return None;
        }
        let template = generate_zap_md_template(project_type);
        match std::fs::write("ZAP.md", &template) {
            Ok(_) => {
                println!("  {} Created ZAP.md for {} project.", "✓".green(), project_type.cyan());
                println!(
                    "  {} Project initialized. zap will remember this project.",
                    "✓".green(),
                );
                println!("  {} Asking the agent to analyse the repo and fill in ZAP.md…", "⚡".bright_yellow());
                Some(
                    "I just created ZAP.md with a template. Please read the project \
                     source files and fill in every section of ZAP.md accurately: \
                     Overview, Build & Test commands, Code Style conventions, Architecture, \
                     Important Files, and Do Not Touch sections. Use edit_file to update ZAP.md \
                     in place with real information from the repo. Also create \
                     .zap/understanding.md with a concise technical summary: main modules, \
                     key data flows, important patterns, and any non-obvious constraints."
                        .to_string(),
                )
            }
            Err(e) => { println!("  {} Could not write ZAP.md: {}", "✗".red(), e); None }
        }
    }

    /// TUI-native init: takes wizard choices, returns (output_text, optional_llm_prompt).
    /// No inquire prompts — all input was collected by the TUI wizard overlay.
    pub fn cmd_init_direct(
        &mut self,
        languages: Vec<String>,
        do_index: bool,
        do_understand: bool,
    ) -> (String, Option<String>) {
        let project_type = detect_project_type();
        let lang_label = if languages.is_empty() {
            project_type.to_string()
        } else {
            languages.join(", ")
        };
        let mut sections: Vec<String> = Vec::new();

        // ── Index ──────────────────────────────────────────────────────────────
        if do_index {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let index_section = match self.code_index.lock() {
                Ok(mut guard) => match guard.index_dir(&cwd) {
                    Ok((new_files, new_syms)) => {
                        crate::project::mark_indexed();
                        let (total_files, total_syms) = guard.total_stats().unwrap_or((new_files, new_syms));
                        // Per-language breakdown from DB
                        let lang_counts = guard.stats_by_language().unwrap_or_default();
                        let db_kb = cwd.join(".zap").join("code.db")
                            .metadata().map(|m| m.len() / 1024).unwrap_or(0);
                        let mut s = format!(
                            "Code index\n  {} files · {} symbols indexed",
                            total_files, total_syms
                        );
                        if !lang_counts.is_empty() {
                            let breakdown: Vec<String> = lang_counts.iter()
                                .map(|(l, n)| format!("{} ({})", l, n))
                                .collect();
                            s.push_str(&format!("\n  Languages: {}", breakdown.join(", ")));
                        }
                        if db_kb > 0 {
                            s.push_str(&format!("\n  DB: {} KB · .zap/code.db", db_kb));
                        }
                        s.push_str("\n  Auto-updates: every 2 min while running · at session end");
                        s.push_str("\n  Run /index any time to refresh manually");
                        s
                    }
                    Err(e) => format!("Index error: {}", e),
                },
                Err(_) => "Index busy — run /index manually".to_string(),
            };
            sections.push(index_section);
        }

        // ── Write project.json ────────────────────────────────────────────────
        let cwd_name = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "project".to_string());
        let meta = crate::project::ProjectMeta {
            name:           cwd_name.clone(),
            language:       languages,
            indexed:        do_index,
            indexed_at:     if do_index { Some(chrono::Utc::now().to_rfc3339()) } else { None },
            initialized_at: Some(chrono::Utc::now().to_rfc3339()),
        };
        if let Err(e) = crate::project::save_project_meta(&meta) {
            sections.push(format!("✗ Could not write project.json: {}", e));
        }

        // ── Create ZAP.md if missing ──────────────────────────────────────────
        let zap_md = std::path::Path::new("ZAP.md");
        let created_zap_md = if !zap_md.exists() {
            let template = generate_zap_md_template(project_type);
            std::fs::write("ZAP.md", &template).is_ok()
        } else {
            false
        };

        // ── Summary block ──────────────────────────────────────────────────────
        let mut created = Vec::new();
        created.push("✓ .zap/project.json — language, index state, timestamps".to_string());
        if created_zap_md {
            created.push("✓ ZAP.md — project instructions loaded into every session".to_string());
        } else {
            created.push("· ZAP.md already exists".to_string());
        }
        if do_understand {
            created.push("✓ .zap/understanding.md — technical deep-dive (being written…)".to_string());
        }
        sections.push(format!("Files\n{}", created.join("\n")));

        // Analysis provenance note — shown whenever indexing ran
        if do_index {
            sections.push(
                "Analysis method\n\
                 Everything above is grounded in your actual source code:\n\
                 ◎ tree-sitter AST — symbols parsed directly from files, not inferred\n\
                 ◎ grep / search  — pattern matches against real file content\n\
                 ◎ file reads     — key files read in full for context\n\
                 The index is deterministic: same code → same results, every time.\n\
                 If you refactor or add files, /index refreshes it instantly."
                .to_string(),
            );
        }

        let output = format!(
            "Project '{}' initialized  ({})\n\n{}",
            cwd_name,
            lang_label,
            sections.join("\n\n")
        );

        // ── Optional LLM prompt to fill ZAP.md ───────────────────────────────
        let llm_prompt = if do_understand {
            Some(
                "I just initialized this project with /init. Your job is to analyse the \
                 codebase and produce two files.\n\
                 \n\
                 METHODOLOGY — use these tools in order, and mention which ones you used \
                 at the start of your response so the user can see the analysis is grounded \
                 in real source:\n\
                 1. Use the code index (search_symbols / code_map) to get a full symbol map\n\
                 2. Use grep / glob_read to find entry points, config files, key patterns\n\
                 3. Read the most important source files in full with read_file\n\
                 4. Only write after you have seen the actual code — no guessing\n\
                 \n\
                 OUTPUT:\n\
                 1. Fill every section of ZAP.md (Overview, Build & Test, Code Style, \
                 Architecture, Important Files, Do Not Touch) with facts from the source.\n\
                 2. Create .zap/understanding.md: main modules, entry points, data flows, \
                 key abstractions, non-obvious constraints, and a one-line summary of each \
                 top-level file/directory.\n\
                 \n\
                 Use edit_file for both files. Be specific — no generic filler. \
                 Start your reply with a brief one-line summary of the tools you used \
                 (e.g. 'Analysed via code index (247 symbols), grep (12 patterns), \
                 read_file (8 files)')."
                .to_string(),
            )
        } else if created_zap_md {
            None
        } else {
            None
        };

        (output, llm_prompt)
    }

    /// Write `.zap/context.md` and append to `.zap/session_log.md` at session end.
    /// Called from all exit paths (REPL, TUI, SDK).
    pub fn save_context(&self) {
        if self.turn_count == 0 {
            return; // nothing happened this session — don't overwrite existing context
        }
        let goal = self.store
            .get_session_goal(self.session_id)
            .unwrap_or_else(|| "(untitled session)".to_string());
        if let Err(e) = crate::project::save_session_context(
            self.session_id,
            &goal,
            &self.files_changed,
        ) {
            crate::log::write("WARN ", &format!("could not write context.md: {}", e));
        }
        if let Err(e) = crate::project::append_session_log(
            self.session_id,
            &goal,
            &self.files_changed,
        ) {
            crate::log::write("WARN ", &format!("could not update session_log.md: {}", e));
        }
        // Auto-create/update understanding.md if it doesn't exist or is the
        // placeholder template (e.g. projects initialised before this feature).
        let (files, symbols, langs): (usize, usize, Vec<(String, usize)>) =
            self.code_index.lock().ok().and_then(|mut guard| {
                let cwd = std::env::current_dir().ok()?;
                let _ = guard.index_dir(&cwd); // re-index so stats are fresh
                let (f, s) = guard.total_stats().ok()?;
                let l = guard.stats_by_language().ok().unwrap_or_default();
                Some((f, s, l))
            }).unwrap_or_default();
        if let Err(e) = crate::project::ensure_understanding_md(
            std::env::current_dir().ok().and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string())),
            files, symbols, &langs,
        ) {
            crate::log::write("WARN ", &format!("could not ensure understanding.md: {}", e));
        }
    }

    pub fn cmd_think(&mut self, arg: &str) {
        match arg.trim() {
            "" | "status" => {
                if self.thinking_budget == 0 {
                    println!("  {} Extended thinking: {}", "◎".truecolor(100, 200, 255), "off".dimmed());
                } else {
                    println!("  {} Extended thinking: {} token budget", "◎".truecolor(100, 200, 255), self.thinking_budget.to_string().cyan());
                }
                println!("  {} Usage: /think on|off|<tokens>  (e.g. /think 8000)", "·".dimmed());
                println!("  {} Note: extended thinking requires claude-3-7-sonnet or newer.", "·".dimmed());
            }
            "off" | "0" => {
                self.thinking_budget = 0;
                println!("  {} Extended thinking {}", "◎".truecolor(100, 200, 255), "disabled".dimmed());
            }
            "on" => {
                self.thinking_budget = 8000;
                println!("  {} Extended thinking {} ({} token budget)", "◎".truecolor(100, 200, 255), "enabled".green(), "8000".cyan());
            }
            n => match n.parse::<u32>() {
                Ok(0) => {
                    self.thinking_budget = 0;
                    println!("  {} Extended thinking {}", "◎".truecolor(100, 200, 255), "disabled".dimmed());
                }
                Ok(v) => {
                    self.thinking_budget = v;
                    println!("  {} Extended thinking {} ({} token budget)", "◎".truecolor(100, 200, 255), "enabled".green(), v.to_string().cyan());
                }
                Err(_) => println!("  {} Usage: /think on|off|<budget_tokens>  e.g. /think 8000", "✗".red()),
            }
        }
    }

    pub fn cmd_index(&mut self, arg: &str) {
        let cwd    = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let target = if arg.is_empty() { cwd.clone() } else { std::path::PathBuf::from(arg) };

        if arg == "clear" || arg == "reset" {
            match crate::code_index::global_index() {
                None => println!("  {} index not initialised", "✗".red()),
                Some(arc) => match arc.lock() {
                    Ok(mut idx) => match idx.clear() {
                        Ok(_) => println!("  {} Index cleared — run {} to rebuild.", "✓".green(), "/index".cyan()),
                        Err(e) => println!("  {} clear failed: {}", "✗".red(), e),
                    },
                    Err(_) => println!("  {} index lock busy", "✗".red()),
                },
            }
            return;
        }

        if arg == "stats" || arg == "status" {
            let (files, syms) = crate::code_index::global_stats();
            let db = cwd.join(".zap").join("code.db");
            let db_kb = db.exists()
                .then(|| std::fs::metadata(&db).ok().map(|m| m.len() / 1024))
                .flatten()
                .unwrap_or(0);

            println!();
            println!("  {} code index", "◎".truecolor(100, 200, 255).bold());
            println!("  {}", "─".repeat(50).truecolor(60, 55, 80));
            println!("  {:<10} {}    {:<10} {}    {:<6} {} KB",
                "files".truecolor(100, 95, 130),  files.to_string().cyan().bold(),
                "symbols".truecolor(100, 95, 130), syms.to_string().cyan().bold(),
                "db".truecolor(100, 95, 130),      db_kb.to_string().dimmed());

            // Symbol breakdown by kind
            let by_kind = crate::code_index::global_stats_by_kind();
            if !by_kind.is_empty() {
                println!();
                println!("  {} by kind", "▸".truecolor(255, 210, 50));
                let max = by_kind.iter().map(|(_, n)| *n).max().unwrap_or(1);
                for (kind, count) in &by_kind {
                    let bar_len = (count * 20 / max).max(1);
                    let bar: String = "█".repeat(bar_len);
                    let pct = count * 100 / syms.max(1);
                    println!("    {:<8} {:>5}  {}  {}%",
                        kind.truecolor(150, 210, 255),
                        count.to_string().dimmed(),
                        bar.truecolor(80, 160, 220),
                        pct.to_string().truecolor(100, 95, 130));
                }
            }

            // Top files
            let top = crate::code_index::global_top_files(8);
            if !top.is_empty() {
                println!();
                println!("  {} top files by symbol count", "▸".truecolor(255, 210, 50));
                for (path, count) in &top {
                    // Strip common prefix for readability
                    let short = path
                        .strip_prefix(cwd.to_str().unwrap_or(""))
                        .unwrap_or(path)
                        .trim_start_matches('/');
                    println!("    {:>4}  {}", count.to_string().cyan(), short.truecolor(140, 135, 160));
                }
            }
            println!();
            return;
        }

        if arg == "files" || arg == "list" {
            let entries = crate::code_index::global_list_indexed_files(200);
            if entries.is_empty() {
                println!("  {} No files indexed yet. Run {} to index the project.", "·".dimmed(), "/index".cyan());
            } else {
                println!("  {} {} file(s) in code index:", "◎".truecolor(100, 200, 255), entries.len());
                for (path, syms) in &entries {
                    println!("  {} {:>4} sym  {}", "·".dimmed(), syms, path.dimmed());
                }
            }
            return;
        }

        if arg == "db" {
            // Show agent.db summary from ~/.zap/agent.db
            let db_path = dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".zap").join("agent.db");
            if !db_path.exists() {
                println!("  {} agent.db not found at {}", "✗".red(), db_path.display());
                return;
            }
            match rusqlite::Connection::open(&db_path) {
                Err(e) => println!("  {} failed to open agent.db: {}", "✗".red(), e),
                Ok(conn) => {
                    let sessions: i64 = conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap_or(0);
                    let memory: i64  = conn.query_row("SELECT COUNT(*) FROM memory", [], |r| r.get(0)).unwrap_or(0);
                    let branches: i64 = conn.query_row("SELECT COUNT(*) FROM branches", [], |r| r.get(0)).unwrap_or(0);
                    println!("  {} agent.db  ({})", "◎".truecolor(100, 200, 255), db_path.display().to_string().dimmed());
                    println!("  {} sessions: {}  memory entries: {}  branches: {}", "·".dimmed(), sessions, memory, branches);

                    // Recent sessions
                    let mut stmt = conn.prepare(
                        "SELECT id, goal, model, created_at FROM sessions ORDER BY id DESC LIMIT 10"
                    ).unwrap();
                    let rows: Vec<(i64, String, String, String)> = stmt.query_map([], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                    }).unwrap().flatten().collect();
                    if !rows.is_empty() {
                        println!("  {} Recent sessions:", "·".dimmed());
                        for (id, goal, model, created) in &rows {
                            let short_goal = if goal.chars().count() > 60 {
                                format!("{}…", goal.chars().take(60).collect::<String>())
                            } else {
                                goal.clone()
                            };
                            println!("    {} #{} [{}] {} — {}", "·".dimmed(), id, model.dimmed(), short_goal.cyan(), created.dimmed());
                        }
                    }

                    // Memory entries
                    let mut mstmt = conn.prepare("SELECT key, value FROM memory ORDER BY key LIMIT 20").unwrap();
                    let mrows: Vec<(String, String)> = mstmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().flatten().collect();
                    if !mrows.is_empty() {
                        println!("  {} Memory ({} entries):", "·".dimmed(), mrows.len());
                        for (key, val) in &mrows {
                            let short_val = if val.chars().count() > 80 {
                                format!("{}…", val.chars().take(80).collect::<String>())
                            } else {
                                val.clone()
                            };
                            println!("    {} {}: {}", "·".dimmed(), key.cyan(), short_val.dimmed());
                        }
                    }
                }
            }
            return;
        }

        if arg == "quality" {
            let cwd_str = cwd.to_string_lossy();
            let shorten = |p: &str| -> String {
                p.strip_prefix(cwd_str.as_ref())
                    .unwrap_or(p)
                    .trim_start_matches('/')
                    .to_string()
            };

            // Ensure ref counts are fresh before reporting
            if let Ok(mut guard) = self.code_index.lock() {
                let _ = guard.compute_reference_counts();
            }

            let report = match crate::code_index::global_quality_report() {
                Some(r) => r,
                None => {
                    println!("  {} index not ready — run {} first", "✗".red(), "/index".cyan());
                    return;
                }
            };

            let score = report.score();
            let score_color = if score >= 80 { score.to_string().green().to_string() }
                              else if score >= 60 { score.to_string().truecolor(255,200,60).to_string() }
                              else { score.to_string().red().to_string() };

            println!();
            println!("  {} code quality — {} files · {} symbols",
                "◎".truecolor(100, 200, 255).bold(),
                report.total_files.to_string().cyan(),
                report.total_syms.to_string().cyan());
            println!("  {}", "─".repeat(60).truecolor(60, 55, 80));

            // God objects
            if !report.god_objects.is_empty() {
                println!();
                println!("  {} god objects  (>15 methods — split recommended)",
                    "⚠".truecolor(255, 140, 60).bold());
                for (label, count, path) in &report.god_objects {
                    let bar: String = "█".repeat((*count / 5).min(20));
                    println!("    {:<28} {} methods  {}  {}",
                        label.truecolor(255, 180, 80).bold(),
                        count.to_string().truecolor(255,140,60),
                        bar.truecolor(255,140,60),
                        shorten(path).truecolor(100, 95, 130));
                }
            }

            // Large files
            if !report.large_files.is_empty() {
                println!();
                println!("  {} large files  (>50 symbols)", "⚠".truecolor(255, 200, 60).bold());
                let max_syms = report.large_files.iter().map(|(_, n)| *n).max().unwrap_or(1);
                for (path, syms) in &report.large_files {
                    let bar_len = (syms * 20 / max_syms).max(1);
                    let bar: String = "█".repeat(bar_len);
                    println!("    {:>5} sym  {}  {}",
                        syms.to_string().cyan(),
                        bar.truecolor(100, 200, 255),
                        shorten(path).truecolor(140, 135, 160));
                }
            }

            // High coupling
            if !report.high_coupling.is_empty() {
                println!();
                println!("  {} high coupling  (many references — risky to change)",
                    "✦".truecolor(200, 150, 255).bold());
                for (name, path, line, refs) in &report.high_coupling {
                    // ref_count includes definition; subtract 1 for call-site estimate
                    let call_sites = refs.saturating_sub(1);
                    println!("    {:<32} {}×  {}:{}",
                        name.truecolor(200, 150, 255).bold(),
                        call_sites.to_string().truecolor(200, 150, 255),
                        shorten(path).truecolor(100, 95, 130),
                        line.to_string().dimmed());
                }
            }

            // Dead code candidates
            if !report.dead_candidates.is_empty() {
                println!();
                println!("  {} dead code candidates  (pub fn, 0 external refs)",
                    "◌".truecolor(130, 125, 150).bold());
                for (name, path, line) in &report.dead_candidates {
                    println!("    {:<32} {}:{}",
                        name.truecolor(130, 125, 150),
                        shorten(path).truecolor(100, 95, 130),
                        line.to_string().dimmed());
                }
            }

            // Complex functions
            if !report.complex_fns.is_empty() {
                println!();
                println!("  {} complex signatures  (truncated — many params or generics)",
                    "◈".truecolor(255, 200, 60).bold());
                for (name, path, line) in &report.complex_fns {
                    println!("    {:<32} {}:{}",
                        name.truecolor(255, 200, 60),
                        shorten(path).truecolor(100, 95, 130),
                        line.to_string().dimmed());
                }
            }

            // Async density
            if !report.async_files.is_empty() {
                println!();
                println!("  {} async density", "⚡".bright_yellow().bold());
                for (path, total, async_n) in &report.async_files {
                    let pct = async_n * 100 / total.max(&1);
                    let bar: String = "█".repeat(pct / 5);
                    println!("    {:>3}%  {}  {}",
                        pct.to_string().truecolor(255, 220, 80),
                        bar.truecolor(255, 200, 60),
                        shorten(path).truecolor(140, 135, 160));
                }
            }

            // Score
            println!();
            println!("  {}", "─".repeat(60).truecolor(60, 55, 80));
            println!("  quality score  {}/100", score_color);
            if score < 80 {
                println!();
                if report.god_objects.iter().any(|(_, n, _)| *n > 30) {
                    println!("  {} largest god object has >30 methods — extract sub-handlers", "→".truecolor(255,140,60));
                }
                if report.large_files.iter().any(|(_, n)| *n > 80) {
                    println!("  {} files with >80 symbols should be split by responsibility", "→".truecolor(255,200,60));
                }
                if !report.dead_candidates.is_empty() {
                    println!("  {} {} pub fn never referenced — check if they can be removed",
                        "→".truecolor(130, 125, 150), report.dead_candidates.len());
                }
            }
            println!();
            return;
        }

        println!("  {} tree-sitter scanning {}…", "◎".truecolor(100, 200, 255), target.display().to_string().cyan());
        if let Ok(mut guard) = self.code_index.lock() {
            match guard.index_dir(&target) {
                Ok((files, syms)) => {
                    println!("  {} tree-sitter: {} file(s) indexed · {} symbol(s) extracted",
                        "✓".green(), files.to_string().cyan(), syms.to_string().cyan());
                    let (total_f, total_s) = guard.total_stats().unwrap_or((0, 0));
                    println!("  {} total in index: {} file(s) · {} symbol(s)", "·".dimmed(), total_f, total_s);
                    crate::project::mark_indexed();
                }
                Err(e) => println!("  {} index error: {}", "✗".red(), e),
            }
        } else {
            println!("  {} index is locked (reindexing in progress?)", "✗".red());
        }
    }
}

// ── Tasks ─────────────────────────────────────────────────────────────────────

impl Session {
    pub async fn cmd_tasks(&mut self) {
        use inquire::Select;
        let cfg = crate::ui::inquire_render_config();

        let task_files = crate::task_planner::discover_task_files();

        if task_files.is_empty() {
            println!("  {} No task sessions found.", "✗".red());
            println!("  {} Start one by selecting {} mode at startup.", "·".dimmed(), "Task".cyan());
            println!("  {} Task files live in {}", "·".dimmed(), ".zap/tasks/<session>/tasks.md".cyan());
            return;
        }

        // ── Step 1: pick task folder ─────────────────────────────────────────
        let folder_labels: Vec<String> = task_files.iter().map(|tf| {
            let done  = tf.done_count();
            let total = tf.tasks.len();
            let bar   = if total == 0 { String::new() } else {
                let filled = (done * 10) / total;
                format!("[{}{}]", "█".repeat(filled), "░".repeat(10 - filled))
            };
            format!("{:<40} {}/{} done  {}  {}",
                tf.folder,
                done, total,
                bar,
                tf.path.display(),
            )
        }).collect();

        let chosen_folder = match Select::new("Task session:", folder_labels.iter().map(|s| s.as_str()).collect())
            .with_render_config(cfg.clone())
            .with_help_message("↑↓ navigate   Enter select   Esc cancel")
            .with_page_size(10)
            .prompt_skippable()
        {
            Ok(Some(s)) => s.to_string(),
            _ => return,
        };

        let tf_idx = match folder_labels.iter().position(|l| l == &chosen_folder) {
            Some(i) => i,
            None    => return,
        };
        let tf = &task_files[tf_idx];

        // ── Step 2: show tasks in this session ───────────────────────────────
        println!();
        println!(
            "  {} {}  {}/{}  {}",
            "◆".truecolor(255,210,50),
            tf.goal.truecolor(255,210,50).bold(),
            tf.done_count(), tf.tasks.len(),
            tf.path.display().to_string().truecolor(100,95,130),
        );
        println!("  {}", "─".repeat(56).truecolor(60,55,80));

        let task_labels: Vec<String> = tf.tasks.iter().map(|t| {
            let icon    = if t.is_done() { "✓".green().to_string() } else { "○".truecolor(150,145,170).to_string() };
            let skill   = t.skill_name.as_deref()
                .map(|s| format!("  [{}]", s))
                .unwrap_or_default();
            let done_steps = t.steps.iter().filter(|(c,_)| *c).count();
            let total_steps = t.steps.len();
            format!("{} {}. {}{}  {}/{}",
                icon,
                t.number,
                t.title,
                skill.truecolor(100,95,130).to_string(),
                done_steps, total_steps,
            )
        }).collect();

        let mut options: Vec<&str> = task_labels.iter().map(|s| s.as_str()).collect();
        options.push("← back");

        let chosen_task = match Select::new("Select task to execute:", options)
            .with_render_config(cfg)
            .with_help_message("↑↓ navigate   Enter execute   Esc cancel")
            .with_page_size(12)
            .prompt_skippable()
        {
            Ok(Some(s)) if s != "← back" => s.to_string(),
            _ => return,
        };

        let task_idx = match task_labels.iter().position(|l| l == &chosen_task) {
            Some(i) => i,
            None    => return,
        };
        let task = &tf.tasks[task_idx];

        if task.is_done() {
            println!("  {} Task {} is already done. Re-run anyway? [y/N] ", "·".dimmed(), task.number);
            let mut ans = String::new();
            std::io::stdin().read_line(&mut ans).ok();
            if !ans.trim().eq_ignore_ascii_case("y") { return; }
        }

        // ── Step 3: execute the selected task ────────────────────────────────
        println!();
        println!("  {} Executing task {}…", "▶".cyan().bold(), task.number);
        if let Err(e) = self.handle_user_turn(&task.execution_prompt()).await {
            println!("  {} {}", "✗".red(), e);
        }
    }
}

// ── Skills ────────────────────────────────────────────────────────────────────

impl Session {
    pub async fn cmd_skill(&mut self, args: &str) {
        let parts: Vec<&str> = args.splitn(2, ' ').collect();
        let subcmd = parts.first().copied().unwrap_or("list");
        let name   = parts.get(1).copied().unwrap_or("").trim();

        match subcmd {
            "list" | "" => {
                let skills = crate::skill_manager::load_all_skills(&self.config.skill_paths);
                if skills.is_empty() {
                    println!("  No skills found.");
                    println!("  Create one: {} or add .md files to .zap/skills/", "/skill create <name>".cyan());
                    return;
                }
                println!();
                println!("  {} {}", "Skills".bold(), format!("({} total)", skills.len()).dimmed());
                println!("  {}", "──────────────────────────────────────────────────────────".dimmed());

                use crate::skill_manager::SkillCategory;
                let groups: &[(&str, SkillCategory)] = &[
                    ("Core  (always injected)", SkillCategory::Core),
                    ("Practice  (always trigger-matchable)", SkillCategory::Practice),
                    ("Domain  (session-scoped)", SkillCategory::Domain),
                ];
                for (label, cat) in groups {
                    let group: Vec<_> = skills.iter().filter(|s| &s.category == cat).collect();
                    if group.is_empty() { continue; }
                    println!("  {} {}", "▸".truecolor(255, 210, 50), label.truecolor(150, 140, 170).bold());
                    for s in &group {
                        let (glyph, src_color) = match &s.source {
                            crate::skill_manager::SkillSource::Project =>
                                ("▶", s.name.truecolor(100, 230, 130).bold()),
                            crate::skill_manager::SkillSource::Global  =>
                                ("●", s.name.truecolor(100, 210, 255).bold()),
                            crate::skill_manager::SkillSource::Bundled =>
                                ("◆", s.name.truecolor(200, 195, 220).bold()),
                            crate::skill_manager::SkillSource::External(_) =>
                                ("◉", s.name.truecolor(255, 200, 100).bold()),
                        };
                        let src_tag = format!("[{}]", crate::skill_manager::source_label(&s.source));
                        let scope_marker = if cat == &SkillCategory::Domain {
                            if self.domain_scope.is_empty() || self.domain_scope.contains(&s.name) {
                                " ✓".truecolor(100, 230, 130).to_string()
                            } else {
                                "  ".to_string()
                            }
                        } else {
                            String::new()
                        };
                        let detail = if s.is_always_on() {
                            "always-on".truecolor(255, 200, 60).to_string()
                        } else {
                            s.triggers.join(", ").truecolor(100, 95, 130).to_string()
                        };
                        let desc = if s.description.is_empty() { String::new() }
                                   else { format!("  {}", s.description.truecolor(120, 115, 140)) };
                        println!("    {} {}{}  {}  ~{}t  {}{}",
                            glyph.truecolor(150, 145, 170),
                            src_color,
                            scope_marker,
                            src_tag.truecolor(100, 95, 130),
                            s.tokens(),
                            detail,
                            desc,
                        );
                    }
                    println!();
                }
                if !self.domain_scope.is_empty() {
                    println!("  {} domain scope: {} — {} to clear or {} to add",
                        "tip:".truecolor(100, 95, 130),
                        self.domain_scope.iter().cloned().collect::<Vec<_>>().join(", ").cyan(),
                        "/skill scope reset".cyan(),
                        "/skill scope add <name>".cyan(),
                    );
                } else {
                    println!("  {} all domain skills active — use {} to restrict",
                        "tip:".truecolor(100, 95, 130),
                        "/skill scope add <name>".cyan(),
                    );
                }
                println!("  {} {} | {} | {} | {} | {}",
                    "tip:".truecolor(100, 95, 130),
                    "◆ built-in".truecolor(200, 195, 220),
                    "● global (~/.zap/skills/)".truecolor(100, 210, 255),
                    "▶ project (.zap/skills/)".truecolor(100, 230, 130),
                    "◉ external (skill_paths in ~/.agent.toml)".truecolor(255, 200, 100),
                    "/skill create <name>".cyan(),
                );
                println!();
            }
            "scope" => {
                let arg = name.trim();
                if arg.is_empty() {
                    // Show current scope
                    println!();
                    if self.domain_scope.is_empty() {
                        println!("  Domain scope: {} (all domain skills are trigger-matchable)", "unrestricted".truecolor(255, 200, 60));
                    } else {
                        let mut names: Vec<_> = self.domain_scope.iter().cloned().collect();
                        names.sort_unstable();
                        println!("  Domain scope: {}", names.join(", ").cyan());
                    }
                    println!("  {} {} {} {}",
                        "tip:".truecolor(100, 95, 130),
                        "/skill scope add <name>  ·".dimmed(),
                        "/skill scope remove <name>  ·".dimmed(),
                        "/skill scope reset".dimmed(),
                    );
                    println!();
                } else if let Some(skill_name) = arg.strip_prefix("add ").map(str::trim) {
                    if self.skills.iter().any(|s| s.name == skill_name && s.category == crate::skill_manager::SkillCategory::Domain) {
                        self.domain_scope.insert(skill_name.to_string());
                        println!("  {} '{}' added to domain scope", "✓".green(), skill_name);
                    } else {
                        println!("  {} no domain skill named '{}'", "✗".red(), skill_name);
                    }
                } else if let Some(skill_name) = arg.strip_prefix("remove ").map(str::trim) {
                    if self.domain_scope.remove(skill_name) {
                        println!("  {} '{}' removed from domain scope", "✓".green(), skill_name);
                    } else {
                        println!("  {} '{}' was not in scope", "✗".red(), skill_name);
                    }
                } else if arg == "reset" {
                    self.domain_scope.clear();
                    println!("  {} domain scope cleared — all domain skills are now trigger-matchable", "✓".green());
                } else {
                    println!("  {} /skill scope  |  scope add <name>  |  scope remove <name>  |  scope reset", "usage:".truecolor(100, 95, 130));
                }
            }
            "show" => {
                if name.is_empty() { println!("  Usage: /skill show <name>"); return; }
                match crate::skill_manager::load_all_skills(&self.config.skill_paths).into_iter().find(|s| s.name == name) {
                    Some(s) => {
                        println!();
                        let trigger_display = if s.is_always_on() {
                            "always-on".to_string()
                        } else {
                            format!("triggers: {}", s.triggers.join(", "))
                        };
                        println!("  {}  [{}]  ~{}t  {}",
                            s.name.cyan().bold(),
                            crate::skill_manager::source_label(&s.source).truecolor(100, 95, 130),
                            s.tokens(),
                            trigger_display.dimmed(),
                        );
                        if !s.description.is_empty() {
                            println!("  {}", s.description.truecolor(120, 115, 140));
                        }
                        if let Some(ref lic) = s.license {
                            println!("  license: {}", lic.dimmed());
                        }
                        println!("  {}", "──────────────────────────────────────────".dimmed());
                        for line in s.content.lines() { println!("  {}", line); }
                        println!();
                    }
                    None => println!("  {} Skill '{}' not found.", "✗".red(), name),
                }
            }
            "export" => {
                let overwrite = name.contains("--overwrite");
                let target    = name.replace("--overwrite", "").trim().to_string();

                if target == "--all" || target.is_empty() && name.contains("--all") {
                    // Export every bundled skill.
                    let skills = crate::skill_manager::load_all_skills(&self.config.skill_paths);
                    let bundled: Vec<_> = skills.iter()
                        .filter(|s| s.source == crate::skill_manager::SkillSource::Bundled)
                        .collect();
                    if bundled.is_empty() {
                        println!("  {} No built-in skills found.", "·".dimmed());
                        return;
                    }
                    let mut ok = 0usize;
                    let mut skipped = 0usize;
                    for skill in &bundled {
                        match crate::skill_manager::export_skill(skill, overwrite) {
                            Ok(path) => {
                                println!("  {} {}", "✓".green(), path.display().to_string().dimmed());
                                ok += 1;
                            }
                            Err(e) if e.to_string().contains("already exists") => {
                                println!("  {} {} (skip — already exported)", "·".dimmed(), skill.name.dimmed());
                                skipped += 1;
                            }
                            Err(e) => println!("  {} {}: {}", "✗".red(), skill.name, e),
                        }
                    }
                    println!("  {} exported {} skill(s){} to {}",
                        "✓".green(), ok,
                        if skipped > 0 { format!(", {} skipped", skipped) } else { String::new() },
                        "~/.zap/skills/".cyan()
                    );
                    println!("  {} Edit the files, then restart zap to pick up changes.", "·".dimmed());
                    self.skills = crate::skill_manager::load_all_skills(&self.config.skill_paths);
                } else if target.is_empty() {
                    println!("  Usage: /skill export <name> [--overwrite]  or  /skill export --all");
                } else {
                    // Export a single named skill.
                    match self.skills.iter().find(|s| s.name == target).cloned() {
                        None => println!("  {} Skill '{}' not found. Run /skill list.", "✗".red(), target),
                        Some(skill) => {
                            match crate::skill_manager::export_skill(&skill, overwrite) {
                                Ok(path) => {
                                    println!("  {} Exported to {}", "✓".green(), path.display().to_string().cyan());
                                    println!("  {} Edit that file, then restart zap — your version will override the built-in.", "·".dimmed());
                                    self.skills = crate::skill_manager::load_all_skills(&self.config.skill_paths);
                                }
                                Err(e) if e.to_string().contains("already exists") => {
                                    println!("  {} {} — run with {} to replace it.",
                                        "·".dimmed(),
                                        e,
                                        format!("/skill export {} --overwrite", target).cyan()
                                    );
                                }
                                Err(e) => println!("  {} {}", "✗".red(), e),
                            }
                        }
                    }
                }
            }
            "create" => {
                if name.is_empty() { println!("  Usage: /skill create <name>"); return; }
                match crate::skill_manager::create_skill(name, true) {
                    Ok(path) => {
                        println!("  {} Created: {}", "✓".green(), path.display().to_string().cyan());
                        self.skills = crate::skill_manager::load_all_skills(&self.config.skill_paths);
                    }
                    Err(e) => println!("  {} {}", "✗".red(), e),
                }
            }
            "capture" => {
                let (skill_name, global) = if name.ends_with("--global") {
                    (name.trim_end_matches("--global").trim(), true)
                } else {
                    (name, false)
                };
                if skill_name.is_empty() { println!("  usage: /skill capture <name> [--global]"); return; }
                if self.messages.len() < 6 { println!("  {} need at least 3 turns to capture a skill", "✗".red()); return; }

                let convo_text: String = self.messages.iter()
                    .filter_map(|m| {
                        let text: String = m.content.iter().filter_map(|b| {
                            if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
                        }).collect::<Vec<_>>().join(" ");
                        if text.trim().is_empty() { None } else { Some(format!("[{}] {}", m.role, text.trim())) }
                    }).collect::<Vec<_>>().join("\n");

                let capture_prompt = format!(
                    "Extract all instructions, preferences, rules, and corrections the user \
                     gave during this conversation. Write them as a concise skill markdown file \
                     with YAML frontmatter. Use this format:\n\
                     ---\nname: {name}\ntrigger: [\"keyword1\", \"keyword2\"]\ntokens: ~500\n---\n\
                     [instructions here]\n\nConversation:\n{convo_text}",
                    name = skill_name, convo_text = convo_text
                );

                println!("  {} Capturing skill from conversation…", "◌".dimmed());
                let mut spinner = Self::make_spinner();
                let pb_clone   = spinner.pb_clone();
                let stop_clone = spinner.stop_signal();
                let before: BeforeOutput = Box::new(move || {
                    stop_clone.store(true, Ordering::Relaxed);
                    pb_clone.finish_and_clear();
                });
                let resp = self.client.send(
                    "You extract reusable instructions from conversations into skill files.",
                    &[Message::user_text(&capture_prompt)], &[], Some(before), 0,
                ).await;
                spinner.finish_and_clear();

                match resp {
                    Ok(r) => {
                        let content = r.content.iter()
                            .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                            .collect::<Vec<_>>().join("\n");
                        match crate::skill_manager::save_captured_skill(skill_name, &content, global) {
                            Ok(path) => {
                                println!("  {} skill saved → {}", "✓".green(), path.display().to_string().cyan());
                                self.skills = crate::skill_manager::load_all_skills(&self.config.skill_paths);
                            }
                            Err(e) => println!("  {} could not save: {}", "✗".red(), e),
                        }
                    }
                    Err(e) => println!("  {} capture failed: {}", "✗".red(), e),
                }
            }
            "use" => {
                if name.is_empty() { println!("  Usage: /skill use <name>"); return; }
                if self.skills.iter().any(|s| s.name == name) {
                    self.pinned_skills.insert(name.to_string());
                    println!("  {} '{}' pinned — injected every turn until /skill unuse {}", "✓".green(), name, name);
                } else {
                    println!("  {} skill '{}' not found. Run /skill list to see available skills.", "✗".red(), name);
                }
            }
            "unuse" | "drop" | "unpin" => {
                if name.is_empty() { println!("  Usage: /skill unuse <name>"); return; }
                if self.pinned_skills.remove(name) {
                    println!("  {} '{}' unpinned", "✓".green(), name);
                } else {
                    println!("  {} '{}' was not pinned", "·".dimmed(), name);
                }
            }
            "log" => {
                if self.skill_trace.is_empty() {
                    println!("  {} no turns yet this session", "·".dimmed());
                    return;
                }
                println!();
                println!("  {} skill trace — {} turn(s) this session",
                    "◆".truecolor(255, 210, 50),
                    self.skill_trace.len());
                println!("  {}", "─".repeat(58).truecolor(60, 55, 80));
                for (turn, preview, skills, reason) in &self.skill_trace {
                    let turn_label = format!("#{:<3}", turn).truecolor(100, 95, 130 as u8);
                    let preview_str = if preview.chars().count() >= 60 {
                        format!("{}…", preview)
                    } else {
                        preview.clone()
                    };
                    if skills.is_empty() {
                        let why = reason.as_deref().unwrap_or("no match");
                        let why_colored = if why == "casual" {
                            why.truecolor(120, 115, 140).to_string()
                        } else {
                            why.truecolor(255, 140, 80).to_string() // "no match" stands out
                        };
                        println!("  {}  {}  {} ({})",
                            turn_label,
                            preview_str.truecolor(140, 135, 160),
                            "→ none".truecolor(100, 95, 130),
                            why_colored);
                    } else {
                        let skill_list = skills.iter()
                            .map(|s| s.as_str().truecolor(100, 210, 255).to_string())
                            .collect::<Vec<_>>()
                            .join(", ");
                        println!("  {}  {}  {} {}",
                            turn_label,
                            preview_str.truecolor(200, 195, 220),
                            "→".truecolor(255, 200, 60),
                            skill_list);
                    }
                }
                println!();
                let no_match_count = self.skill_trace.iter()
                    .filter(|(_, _, s, r)| s.is_empty() && r.as_deref() == Some("no match"))
                    .count();
                if no_match_count > 0 {
                    println!("  {} {} turn(s) had no skill match — review triggers with {}",
                        "tip:".truecolor(255, 200, 60),
                        no_match_count.to_string().truecolor(255, 140, 80),
                        "/skill list".cyan());
                }
            }
            _ => println!("  {} /skill list | log | show <name> | export <name|--all> | use <name> | unuse <name> | scope [add|remove|reset] | create <name> | capture <name> [--global]", "✗".red()),
        }
    }
}

// ── Skill helpers (pure text, used by TUI inline handler) ────────────────────

/// Build a plain-text skill listing for inline TUI display (no ANSI escape codes).
pub fn skill_list_text(
    skills: &[crate::skill_manager::Skill],
    domain_scope: &std::collections::HashSet<String>,
    pinned_skills: &std::collections::HashSet<String>,
) -> String {
    use crate::skill_manager::{SkillCategory, SkillSource, source_label};
    let mut s = String::new();
    if skills.is_empty() {
        s.push_str("No skills found.\nCreate one: /skill create <name> or add .md files to .zap/skills/");
        return s;
    }
    s.push_str(&format!("## Skills ({} total)\n\n", skills.len()));
    let groups: &[(&str, SkillCategory)] = &[
        ("Core — always injected", SkillCategory::Core),
        ("Practice — always trigger-matchable", SkillCategory::Practice),
        ("Domain — session-scoped", SkillCategory::Domain),
    ];
    for (label, cat) in groups {
        let group: Vec<_> = skills.iter().filter(|sk| &sk.category == cat).collect();
        if group.is_empty() { continue; }
        s.push_str(&format!("### {}\n", label));
        for sk in &group {
            let src = source_label(&sk.source);
            let src_icon = match &sk.source {
                SkillSource::Project  => "▶",
                SkillSource::Global   => "●",
                SkillSource::Bundled  => "◆",
                SkillSource::External(_) => "◉",
            };
            let pin_mark = if pinned_skills.contains(&sk.name) { " 📌" } else { "" };
            let scope_mark = if *cat == SkillCategory::Domain {
                if domain_scope.is_empty() || domain_scope.contains(&sk.name) { " ✓" } else { "" }
            } else { "" };
            let trigger_info = if sk.is_always_on() {
                "always-on".to_string()
            } else {
                sk.triggers.join(", ")
            };
            s.push_str(&format!(
                "{} **{}**{}{}  [{}]  ~{}t  {}\n",
                src_icon, sk.name, scope_mark, pin_mark, src, sk.tokens(), trigger_info,
            ));
            if !sk.description.is_empty() {
                s.push_str(&format!("  {}\n", sk.description));
            }
        }
        s.push('\n');
    }
    // Scope hint
    if !domain_scope.is_empty() {
        let names: Vec<&str> = domain_scope.iter().map(String::as_str).collect();
        s.push_str(&format!("Scope: {}  ·  /skill scope reset to clear\n", names.join(", ")));
    } else {
        s.push_str("All domain skills active  ·  /skill scope add <name> to restrict\n");
    }
    // Pinned hint
    if !pinned_skills.is_empty() {
        let names: Vec<&str> = pinned_skills.iter().map(String::as_str).collect();
        s.push_str(&format!("Pinned: {}  ·  /skill unuse <name> to remove\n", names.join(", ")));
    }
    s.push_str("\n◆ built-in  ● global (~/.zap/skills/)  ▶ project (.zap/skills/)  ◉ external\n");
    s.push_str("/skill use <name> · /skill show <name> · /skill create <name>");
    s
}

/// Build plain-text for a single skill's detail view.
pub fn skill_show_text(skill: &crate::skill_manager::Skill) -> String {
    use crate::skill_manager::source_label;
    let mut s = String::new();
    let trigger_display = if skill.is_always_on() {
        "always-on".to_string()
    } else {
        format!("triggers: {}", skill.triggers.join(", "))
    };
    s.push_str(&format!("## {}  [{}]  ~{}t\n", skill.name, source_label(&skill.source), skill.tokens()));
    if !skill.description.is_empty() {
        s.push_str(&format!("{}\n", skill.description));
    }
    s.push_str(&format!("{}\n", trigger_display));
    if let Some(ref lic) = skill.license {
        s.push_str(&format!("license: {}\n", lic));
    }
    s.push_str("─────────────────────────────────────────\n");
    s.push_str(&skill.content);
    s
}

// ── Deploy ────────────────────────────────────────────────────────────────────

impl Session {
    /// Run `scripts/deploy.sh` with live streaming output — no LLM involved.
    /// Streams stdout+stderr line-by-line so the terminal never appears frozen.
    pub async fn cmd_deploy(&self, arg: &str) {
        use tokio::io::AsyncBufReadExt;

        let script = "scripts/deploy.sh";
        if !std::path::Path::new(script).exists() {
            println!("  {} {} not found", "✗".red(), script.cyan());
            return;
        }

        let args: Vec<&str> = if arg.is_empty() { vec![] } else { arg.split_whitespace().collect() };
        let label = if args.contains(&"--check") { "deploy --check" } else { "deploy" };

        println!();
        println!("  {} {}", "⚡".bright_yellow(), label.bold());
        println!("  {}", "─".repeat(44).truecolor(60, 55, 80));

        let mut child = match tokio::process::Command::new("bash")
            .arg(script)
            .args(&args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => { println!("  {} failed to start: {}", "✗".red(), e); return; }
        };

        // Merge stdout and stderr by reading both concurrently.
        let stdout = child.stdout.take().map(tokio::io::BufReader::new);
        let stderr = child.stderr.take().map(tokio::io::BufReader::new);

        let print_line = |line: &str| {
            println!("  {}", line);
        };

        match (stdout, stderr) {
            (Some(mut out), Some(mut err)) => {
                let mut out_lines = out.lines();
                let mut err_lines = err.lines();
                loop {
                    tokio::select! {
                        line = out_lines.next_line() => match line {
                            Ok(Some(l)) => print_line(&l),
                            Ok(None) => break,
                            Err(_) => break,
                        },
                        line = err_lines.next_line() => match line {
                            Ok(Some(l)) => print_line(&l),
                            Ok(None) => {},
                            Err(_) => {},
                        },
                    }
                }
                // Drain remaining stderr
                while let Ok(Some(l)) = err_lines.next_line().await {
                    print_line(&l);
                }
            }
            _ => {}
        }

        match child.wait().await {
            Ok(status) if status.success() => {
                println!("  {}", "─".repeat(44).truecolor(60, 55, 80));
                println!("  {} done", "✓".green());
            }
            Ok(status) => {
                println!("  {}", "─".repeat(44).truecolor(60, 55, 80));
                println!("  {} exited with status {}", "✗".red(), status.code().unwrap_or(-1));
            }
            Err(e) => println!("  {} wait error: {}", "✗".red(), e),
        }
        println!();
    }
}

// ── Workflow ──────────────────────────────────────────────────────────────────

impl Session {
    pub async fn cmd_run_workflow(&mut self, name: &str) -> Result<()> {
        let workflow = crate::workflow::load_workflow(name)?;
        println!();
        println!("  {} {} — {}",
            "⚡".bright_yellow(),
            format!("workflow: {}", workflow.name).bold(),
            workflow.description.dimmed());
        println!("  {} {} step(s)", "◌".dimmed(), workflow.steps.len());
        println!();

        for (i, step) in workflow.steps.iter().enumerate() {
            println!("  {} step {}/{}{}",
                "▶".cyan(), i + 1, workflow.steps.len(),
                if step.skill.is_empty() { String::new() }
                else { format!("  [skill: {}]", step.skill).dimmed().to_string() });

            if step.requires_approval {
                use std::io::Write;
                print!("  Continue? [y/N] ");
                std::io::stdout().flush()?;
                let mut line = String::new();
                std::io::stdin().read_line(&mut line)?;
                if !matches!(line.trim().to_lowercase().as_str(), "y" | "yes") {
                    println!("  {} Stopped at step {}.", "✗".red(), i + 1);
                    return Ok(());
                }
            }

            let prompt = if step.skill.is_empty() { step.prompt.clone() }
                         else { format!("[Using skill: {}]\n{}", step.skill, step.prompt) };

            if let Err(e) = self.handle_user_turn(&prompt).await {
                println!("  {} step {} failed: {}", "✗".red(), i + 1, e);
                return Err(e);
            }
        }
        println!("  {} workflow '{}' complete.", "✓".green(), workflow.name.cyan());
        Ok(())
    }
}

// ── Branches ─────────────────────────────────────────────────────────────────

impl Session {
    pub async fn cmd_branch(&mut self, name: &str) {
        if name.is_empty() { println!("  usage: /branch <name>"); return; }
        let json = match serde_json::to_string(&self.messages) {
            Ok(j) => j, Err(e) => { println!("  {} {}", "✗".red(), e); return; }
        };
        match self.store.save_branch(self.session_id, name, &self.current_branch, &json, self.turn_count) {
            Ok(_) => {
                let old = self.current_branch.clone();
                self.current_branch = name.to_string();
                println!("  {} branched from {} → {}", "✓".green(), old.dimmed(), name.cyan().bold());
                println!("  {} conversation forked — changes stay on '{}' until you /switch", "·".dimmed(), name.cyan());
            }
            Err(e) => println!("  {} {}", "✗".red(), e),
        }
    }

    pub fn cmd_branches(&self) {
        match self.store.list_branches(self.session_id) {
            Ok(branches) if branches.is_empty() => {
                println!("  No branches (only main). Create one with /branch <name>");
            }
            Ok(branches) => {
                println!();
                for (name, parent, turns, _) in &branches {
                    let marker = if name == &self.current_branch { " ← current".green().to_string() } else { String::new() };
                    println!("  {}  {}  {} turns  from: {}{}", "·".dimmed(), name.cyan().bold(), turns, parent.dimmed(), marker);
                }
                println!();
            }
            Err(e) => println!("  {} {}", "✗".red(), e),
        }
    }

    pub async fn cmd_switch(&mut self, name: &str) {
        if name.is_empty() { println!("  usage: /switch <branch-name>"); return; }
        let target = name.to_string();

        if let Ok(json) = serde_json::to_string(&self.messages) {
            let _ = self.store.save_branch(self.session_id, &self.current_branch, "main", &json, self.turn_count);
        }

        if target == "main" {
            match self.store.load_messages(self.session_id) {
                Ok(Some(json)) => {
                    if let Ok(msgs) = serde_json::from_str(&json) {
                        let old = self.current_branch.clone();
                        self.messages       = msgs;
                        self.turn_count     = self.messages.iter().filter(|m| m.role == "user").count();
                        self.current_branch = "main".to_string();
                        println!("  {} switched {} → main", "✓".green(), old.dimmed());
                    }
                }
                _ => println!("  {} main branch state not found", "✗".red()),
            }
        } else {
            match self.store.load_branch(self.session_id, &target) {
                Ok(Some((json, turns))) => {
                    if let Ok(msgs) = serde_json::from_str(&json) {
                        let old = self.current_branch.clone();
                        self.messages       = msgs;
                        self.turn_count     = turns;
                        self.current_branch = target.clone();
                        println!("  {} switched {} → {}", "✓".green(), old.dimmed(), target.cyan().bold());
                    }
                }
                Ok(None) => println!("  {} branch '{}' not found", "✗".red(), target),
                Err(e)   => println!("  {} {}", "✗".red(), e),
            }
        }
    }

    pub async fn cmd_merge(&mut self, name: &str) {
        if name.is_empty() { println!("  usage: /merge <branch-name>"); return; }

        let branch_msgs: Vec<Message> = match self.store.load_branch(self.session_id, name) {
            Ok(Some((json, _))) => match serde_json::from_str(&json) {
                Ok(m) => m,
                Err(e) => { println!("  {} could not parse branch: {}", "✗".red(), e); return; }
            },
            Ok(None) => { println!("  {} branch '{}' not found", "✗".red(), name); return; }
            Err(e)   => { println!("  {} {}", "✗".red(), e); return; }
        };

        let branch_text: String = branch_msgs.iter()
            .filter_map(|m| {
                let t: String = m.content.iter().filter_map(|b| {
                    if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
                }).collect::<Vec<_>>().join(" ");
                if t.trim().is_empty() { None } else { Some(format!("[{}] {}", m.role, t.trim())) }
            }).collect::<Vec<_>>().join("\n");

        let prompt = format!("Summarize this conversation branch in 3 sentences, focusing on conclusions and decisions made:\n\n{}", branch_text);

        println!("  {} summarizing branch '{}'…", "◌".dimmed(), name);
        let mut spinner = Self::make_spinner();
        let pb_clone   = spinner.pb_clone();
        let stop_clone = spinner.stop_signal();
        let before: BeforeOutput = Box::new(move || {
            stop_clone.store(true, Ordering::Relaxed);
            pb_clone.finish_and_clear();
        });
        let resp = self.client.send(
            "You summarize conversations concisely.",
            &[Message::user_text(&prompt)], &[], Some(before), 0,
        ).await;
        spinner.finish_and_clear();

        match resp {
            Ok(r) => {
                let summary = r.content.iter()
                    .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                    .collect::<Vec<_>>().join("\n");
                let merge_msg = format!("[merged from branch '{}']\n{}", name, summary);
                self.messages.push(Message {
                    role:    "assistant".to_string(),
                    content: vec![ContentBlock::Text { text: merge_msg }],
                });
                println!("  {} merged '{}' into '{}'", "✓".green(), name.cyan(), self.current_branch.cyan().bold());
                println!("  {}", summary.dimmed());
            }
            Err(e) => println!("  {} merge summary failed: {}", "✗".red(), e),
        }
    }
}

// ── /init helpers (no Session dependency) ────────────────────────────────────

pub fn detect_project_type() -> &'static str {
    if std::path::Path::new("Cargo.toml").exists()            { return "rust"; }
    if std::path::Path::new("go.mod").exists()                { return "go"; }
    if std::path::Path::new("package.json").exists() {
        if std::path::Path::new("tsconfig.json").exists()     { return "typescript"; }
        return "javascript";
    }
    if std::path::Path::new("pyproject.toml").exists()
        || std::path::Path::new("setup.py").exists()
        || std::path::Path::new("requirements.txt").exists()  { return "python"; }
    if std::path::Path::new("pom.xml").exists()               { return "java"; }
    if std::path::Path::new("build.gradle").exists()
        || std::path::Path::new("build.gradle.kts").exists()  { return "kotlin"; }
    if std::path::Path::new("*.swift").exists()
        || std::path::Path::new("Package.swift").exists()     { return "swift"; }
    if std::path::Path::new("CMakeLists.txt").exists()
        || std::path::Path::new("Makefile").exists() {
        // Check for C++ vs C
        if std::fs::read_dir(".")
            .ok()
            .map(|d| d.filter_map(|e| e.ok())
                .any(|e| e.path().extension()
                    .map(|x| x == "cpp" || x == "cc" || x == "cxx")
                    .unwrap_or(false)))
            .unwrap_or(false)
        {
            return "c++";
        }
        return "c";
    }
    // No build file found — ask the user
    ""
}

pub(super) fn generate_zap_md_template(project_type: &str) -> String {
    let (build_cmd, test_cmd, lint_cmd) = match project_type {
        "rust"       => ("cargo build",       "cargo test",      "cargo clippy"),
        "go"         => ("go build ./...",    "go test ./...",   "golangci-lint run"),
        "typescript" => ("npm run build",     "npm test",        "npm run lint"),
        "javascript" => ("npm run build",     "npm test",        "npm run lint"),
        "python"     => ("pip install -e .",  "pytest",          "ruff check ."),
        "java"       => ("mvn compile",       "mvn test",        "mvn checkstyle:check"),
        "kotlin"     => ("./gradlew build",   "./gradlew test",  "./gradlew lint"),
        "swift"      => ("swift build",       "swift test",      "swiftlint"),
        "c++"        => ("cmake --build .",   "ctest",           "clang-tidy"),
        "c"          => ("make",              "make test",       "clang-tidy"),
        _            => ("make",              "make test",       "make lint"),
    };
    format!(
        r#"# Project Instructions

## Overview
<!-- Describe what this project does in 1-3 sentences. -->

## Build & Test
```
{build}
{test}
{lint}
```

## Code Style
<!-- List any conventions zap must follow: naming, formatting, imports, etc. -->

## Architecture
<!-- Briefly describe the module layout and main data-flow so zap has context. -->

## Important Files
<!-- List key files or directories zap should know about. -->

## Do Not Touch
<!-- List files, directories, or patterns that must not be modified without explicit approval. -->

## Notes
<!-- Anything else zap should know. -->
"#,
        build = build_cmd, test = test_cmd, lint = lint_cmd,
    )
}
