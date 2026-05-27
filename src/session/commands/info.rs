use colored::Colorize;
use crate::{
    audit,
    config::{PermissionMode, Provider},
};
use super::super::Session;

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
        if !self.config.context_paths.is_empty() {
            kv("context_paths", &self.config.context_paths.join(", "));
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
        let n: usize = arg.trim().parse().unwrap_or(20).clamp(1, 500);
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
