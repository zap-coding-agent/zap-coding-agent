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
            ]),
            ("code", &[
                ("/tasks",                   "browse & execute task sessions (.zap/tasks/)"),
                ("/index [path|stats]",      "reindex AST code symbols"),
                ("/undo [file]",             "undo last file edit"),
                ("/init",                    "create CLAUDE.md for this project"),
                ("/run <workflow>",          "run a workflow from .zap/workflows/"),
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
                ("/skill list",              "list available skills"),
                ("/skill scope",             "show/change domain skill scope for this session"),
                ("/skill show <name>",       "preview a skill"),
                ("/skill create <name>",     "create a skill file"),
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
        let url  = self.base_url.as_deref().unwrap_or("https://api.anthropic.com");
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

    pub async fn cmd_compact(&mut self) {
        if self.messages.is_empty() {
            println!("  {} Nothing to compact.", "✗".red());
            return;
        }
        let mut spinner = Self::make_spinner();
        let mut temp = self.messages.clone();
        temp.push(Message::user_text(
            "Please provide a concise summary of this conversation so far, \
             including the key decisions, changes made, and current state. \
             This will replace the conversation history.",
        ));

        let result = self.client.send(
            "You are a helpful assistant. Summarize the conversation concisely.",
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
                println!("  {} Compacted {} messages into a summary.", "✓".green(), turn_count);
            }
            Err(e) => println!("  {} Compact failed: {}", "✗".red(), e),
        }
    }
}

// ── Sessions ──────────────────────────────────────────────────────────────────

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
            match self.store.load_messages(*session_id) {
                Ok(Some(json)) => match serde_json::from_str::<Vec<Message>>(&json) {
                    Ok(msgs) => {
                        self.messages   = msgs;
                        self.turn_count = self.messages.iter().filter(|m| m.role == "user").count();
                        println!("  {} Loaded session #{} — {} messages, model was {}",
                            "✓".green(), session_id, self.messages.len().to_string().cyan(), model.dimmed());
                        println!("  {} {}", "◌".dimmed(), goal.dimmed());
                    }
                    Err(e) => println!("  {} Could not parse messages: {}", "✗".red(), e),
                },
                Ok(None) => println!("  {} No message history saved for that session.", "✗".red()),
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
            name:        &'static str,
            hint:        &'static str,
            kind:        ProviderKind,
            models:      &'static [&'static str],
            base_url:    Option<&'static str>,
            needs_key:   bool,
            coming_soon: bool,
        }
        #[derive(Clone)]
        enum ProviderKind { Anthropic, OpenAi }

        let providers: Vec<ProviderDef> = vec![
            ProviderDef { name: "LM Studio",                  hint: "local · OpenAI-compatible",                kind: ProviderKind::OpenAi,    models: &["gemma-4-e4b-it", "qwen2.5-coder-7b-instruct", "mistral-7b-instruct", "Other…"],    base_url: Some("http://localhost:1234"),                                    needs_key: false, coming_soon: false },
            ProviderDef { name: "Ollama",                     hint: "local · OpenAI-compatible",                kind: ProviderKind::OpenAi,    models: &["llama3.2", "llama3.1:70b", "codellama", "qwen2.5-coder", "Other…"],                 base_url: Some("http://localhost:11434"),                                   needs_key: false, coming_soon: false },
            ProviderDef { name: "Anthropic",                  hint: "claude-sonnet-4-6 / claude-opus-4-7",      kind: ProviderKind::Anthropic, models: &["claude-sonnet-4-6", "claude-opus-4-7", "claude-haiku-4-5", "Other…"],               base_url: None,                                                            needs_key: true,  coming_soon: false },
            ProviderDef { name: "Claude Code (Pro/Max API)",  hint: "full API via subscription · after 16 Jun 2026", kind: ProviderKind::Anthropic, models: &["claude-sonnet-4-6", "claude-opus-4-7"],                                     base_url: None,                                                            needs_key: false, coming_soon: true  },
            ProviderDef { name: "OpenAI",                     hint: "gpt-4o / gpt-4o-mini / o3",               kind: ProviderKind::OpenAi,    models: &["gpt-4o", "gpt-4o-mini", "o3", "o4-mini", "Other…"],                                 base_url: None,                                                            needs_key: true,  coming_soon: false },
            ProviderDef { name: "Google Gemini",              hint: "gemini-2.5-pro / gemini-2.0-flash",       kind: ProviderKind::OpenAi,    models: &["gemini-2.0-flash", "gemini-2.5-pro", "gemini-2.5-flash", "Other…"],                 base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai"), needs_key: true,  coming_soon: false },
            ProviderDef { name: "DeepSeek",                   hint: "deepseek-chat / deepseek-reasoner",        kind: ProviderKind::OpenAi,    models: &["deepseek-chat", "deepseek-reasoner", "Other…"],                                     base_url: Some("https://api.deepseek.com"),                                needs_key: true,  coming_soon: false },
            ProviderDef { name: "Groq",                       hint: "llama-3.3-70b · fastest inference",        kind: ProviderKind::OpenAi,    models: &["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "mixtral-8x7b-32768", "Other…"], base_url: Some("https://api.groq.com/openai"),                             needs_key: true,  coming_soon: false },
            ProviderDef { name: "Mistral",                    hint: "mistral-large / codestral",                kind: ProviderKind::OpenAi,    models: &["mistral-large-latest", "codestral-latest", "mistral-small-latest", "Other…"],       base_url: Some("https://api.mistral.ai/v1"),                               needs_key: true,  coming_soon: false },
            ProviderDef { name: "xAI (Grok)",                 hint: "grok-3 / grok-3-mini",                    kind: ProviderKind::OpenAi,    models: &["grok-3", "grok-3-mini", "grok-2", "Other…"],                                       base_url: Some("https://api.x.ai/v1"),                                     needs_key: true,  coming_soon: false },
            ProviderDef { name: "Together AI",                hint: "Llama / Qwen / Mistral open models",       kind: ProviderKind::OpenAi,    models: &["meta-llama/Llama-3-70b-chat-hf", "Qwen/Qwen2.5-72B-Instruct-Turbo", "Other…"],    base_url: Some("https://api.together.xyz/v1"),                             needs_key: true,  coming_soon: false },
            ProviderDef { name: "Perplexity",                 hint: "sonar-pro · web-grounded answers",         kind: ProviderKind::OpenAi,    models: &["sonar-pro", "sonar", "sonar-reasoning", "Other…"],                                  base_url: Some("https://api.perplexity.ai"),                               needs_key: true,  coming_soon: false },
            ProviderDef { name: "Cohere",                     hint: "command-r-plus",                           kind: ProviderKind::OpenAi,    models: &["command-r-plus", "command-r", "Other…"],                                            base_url: Some("https://api.cohere.ai/compatibility/v1"),                  needs_key: true,  coming_soon: false },
            ProviderDef { name: "Custom (OpenAI-compatible)", hint: "any OpenAI-compatible endpoint",           kind: ProviderKind::OpenAi,    models: &["Other…"],                                                                           base_url: None,                                                            needs_key: false, coming_soon: false },
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

        let base_url = if def.name == "Custom (OpenAI-compatible)" {
            match Text::new("Base URL (e.g. http://localhost:8080/v1):").prompt_skippable() {
                Ok(Some(u)) if !u.trim().is_empty() => Some(u.trim().to_string()),
                _ => { println!("  Cancelled."); return; }
            }
        } else {
            def.base_url.map(str::to_string)
        };

        let api_key = if def.needs_key {
            match Text::new("API key (leave blank to keep existing):")
                .with_render_config(cfg.clone())
                .with_help_message("Saved to ~/.agent.toml")
                .prompt_skippable()
            {
                Ok(Some(k)) if !k.trim().is_empty() => k.trim().to_string(),
                _ => config.api_key.clone(),
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

        let mut new_config   = config.clone();
        new_config.provider  = match def.kind { ProviderKind::Anthropic => Provider::Anthropic, ProviderKind::OpenAi => Provider::OpenAi };
        new_config.model     = model_input.clone();
        new_config.base_url  = base_url;
        new_config.api_key   = api_key;

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
            Some(b) => format!("{}/v1/models", b.trim_end_matches('/')),
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
        let tmp = "/tmp/zap_clipboard_paste.png";
        let ok = std::process::Command::new("pngpaste")
            .arg(tmp).status().map(|s| s.success()).unwrap_or(false);
        let ok = ok || {
            let script = format!(
                r#"try
  set d to (the clipboard as «class PNGf»)
  set f to open for access POSIX file "{}" with write permission
  set eof f to 0
  write d to f
  close access f
  return true
on error
  return false
end try"#,
                tmp
            );
            std::process::Command::new("osascript")
                .args(["-e", &script])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "true")
                .unwrap_or(false)
        };

        if ok && std::path::Path::new(tmp).exists() {
            self.cmd_attach(tmp);
        } else {
            println!("  {} No image in clipboard. Copy a screenshot first, then run /paste.", "✗".red());
            println!("  {} You can also use {} to stage a file directly.", "·".dimmed(), "/attach <path>".cyan());
        }
    }

    pub fn cmd_attach(&mut self, path: &str) {
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

// ── Code ──────────────────────────────────────────────────────────────────────

impl Session {
    pub fn cmd_init(&self) -> Option<String> {
        let claude_md = std::path::Path::new("CLAUDE.md");
        if claude_md.exists() {
            println!("  {} CLAUDE.md already exists.", "✗".red());
            return None;
        }
        let project_type = detect_project_type();
        let template     = generate_claude_md_template(project_type);
        match std::fs::write("CLAUDE.md", &template) {
            Ok(_) => {
                println!("  {} Created CLAUDE.md for {} project.", "✓".green(), project_type.cyan());
                println!("  {} Asking the agent to analyse the repo and fill it in…", "⚡".bright_yellow());
                Some(
                    "I just created CLAUDE.md with a template. Please read the project \
                     source files and fill in every section of CLAUDE.md accurately: \
                     Overview, Build & Test commands, Code Style conventions, Architecture, \
                     Important Files, and Do Not Touch sections. Use edit_file to update CLAUDE.md \
                     in place with real information from the repo."
                        .to_string(),
                )
            }
            Err(e) => { println!("  {} Could not write CLAUDE.md: {}", "✗".red(), e); None }
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

        if arg == "stats" || arg == "status" {
            let (files, syms) = crate::code_index::global_stats();
            println!("  {} {} file(s) indexed, {} symbol(s)", "◎".truecolor(100, 200, 255), files, syms);
            let db = cwd.join(".zap").join("code.db");
            if db.exists() {
                if let Ok(meta) = std::fs::metadata(&db) {
                    println!("  {} db: {} KB", "·".dimmed(), meta.len() / 1024);
                }
            }
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

        println!("  {} Indexing {}…", "◎".truecolor(100, 200, 255), target.display().to_string().cyan());
        if let Ok(mut guard) = self.code_index.lock() {
            match guard.index_dir(&target) {
                Ok((files, syms)) => {
                    println!("  {} indexed {} file(s), {} symbol(s)",
                        "✓".green(), files.to_string().cyan(), syms.to_string().cyan());
                    let (total_f, total_s) = guard.total_stats().unwrap_or((0, 0));
                    println!("  {} total: {} file(s), {} symbol(s)", "·".dimmed(), total_f, total_s);
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
            _ => println!("  {} /skill list | scope [add|remove|reset] | show <name> | create <name> | capture <name> [--global]", "✗".red()),
        }
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

pub(super) fn detect_project_type() -> &'static str {
    if std::path::Path::new("Cargo.toml").exists()   { return "rust"; }
    if std::path::Path::new("go.mod").exists()        { return "go"; }
    if std::path::Path::new("package.json").exists()  { return "node"; }
    if std::path::Path::new("pyproject.toml").exists()
        || std::path::Path::new("setup.py").exists()  { return "python"; }
    if std::path::Path::new("pom.xml").exists()       { return "java/maven"; }
    if std::path::Path::new("build.gradle").exists()  { return "java/gradle"; }
    "generic"
}

pub(super) fn generate_claude_md_template(project_type: &str) -> String {
    let (build_cmd, test_cmd, lint_cmd) = match project_type {
        "rust"        => ("cargo build",     "cargo test",   "cargo clippy"),
        "go"          => ("go build ./...",  "go test ./...", "golint ./..."),
        "node"        => ("npm run build",   "npm test",     "npm run lint"),
        "python"      => ("pip install -e .", "pytest",      "ruff check ."),
        "java/maven"  => ("mvn compile",     "mvn test",     "mvn checkstyle:check"),
        "java/gradle" => ("./gradlew build", "./gradlew test", "./gradlew lint"),
        _             => ("make",            "make test",    "make lint"),
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
