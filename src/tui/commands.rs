/// Native TUI slash-command handlers.
///
/// Returns `Some(text)` for commands handled inline (text shown in the chat area).
/// Returns `None` for commands that need interactive terminal I/O — caller will
/// suspend the TUI, run the command, and show a "press Enter" prompt.
use crate::config::{Config, PermissionMode, Provider};
use crate::session::Session;

/// Full list of slash commands shown in the command picker.
pub const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/help",              "show help"),
    ("/config",            "provider, model, URL"),
    ("/cost",              "token usage & estimated cost"),
    ("/history",           "message count"),
    ("/clear",             "clear conversation history"),
    ("/compact",           "summarize and compress history"),
    ("/sessions",          "browse and resume old sessions"),
    ("/cd",                "change working directory"),
    ("/model",             "switch model for this session"),
    ("/models",            "list available models"),
    ("/provider",          "switch provider interactively"),
    ("/permissions",       "change permission mode"),
    ("/tasks",             "browse & execute task sessions"),
    ("/index",             "reindex AST code symbols"),
    ("/undo",              "undo last file edit"),
    ("/init",              "create CLAUDE.md for this project"),
    ("/run",               "run a workflow"),
    ("/memory",            "manage memory entries"),
    ("/skill",             "list available skills"),
    ("/audit",             "show last N audit log lines"),
    ("/hooks",             "list configured hooks"),
    ("/mcp",               "view/edit MCP server configs"),
    ("/branch",            "create a conversation branch"),
    ("/branches",          "list conversation branches"),
    ("/switch",            "switch conversation branch"),
    ("/merge",             "merge a conversation branch"),
    ("/exit",              "quit zap"),
];

/// Return commands whose name starts with `input` (case-insensitive).
/// Always returns at least all commands when `input` == "/".
pub fn filter_commands(input: &str) -> Vec<(&'static str, &'static str)> {
    let lower = input.to_lowercase();
    SLASH_COMMANDS.iter()
        .filter(|(cmd, _)| cmd.starts_with(lower.as_str()))
        .copied()
        .collect()
}

pub fn handle_inline(
    session: &mut Session,
    input: &str,
    config: &Config,
) -> Option<String> {
    let parts: Vec<&str> = input.splitn(2, ' ').collect();
    let cmd  = parts[0];
    let arg  = parts.get(1).copied().unwrap_or("").trim();

    match cmd {
        "/help"    => Some(help_text()),
        "/config"  => Some(config_text(session)),
        "/cost"    => Some(cost_text(session)),
        "/history" => Some(format!(
            "{} messages in history  ·  {} turns this session",
            session.messages.len(), session.turn_count
        )),
        "/clear" => {
            session.messages.clear();
            Some("History cleared.".to_string())
        }
        "/model" if !arg.is_empty() => {
            session.model = arg.to_string();
            let mut nc = config.clone();
            nc.model = arg.to_string();
            session.client = crate::llm_client::create_client(&nc);
            Some(format!("Model switched to: {}", arg))
        }
        "/permissions" => {
            let new_mode = match arg.to_lowercase().as_str() {
                "ask"  => PermissionMode::Ask,
                "auto" => PermissionMode::Auto,
                "deny" => PermissionMode::Deny,
                _ => return Some("Usage: /permissions ask|auto|deny".to_string()),
            };
            session.permissions.mode = new_mode;
            Some(format!("Permission mode: {}", arg))
        }
        "/cd" => {
            if arg.is_empty() {
                Some(format!(
                    "Usage: /cd <path>\nCurrent directory: {}",
                    std::env::current_dir().map(|p| p.display().to_string())
                        .unwrap_or_else(|_| "?".to_string())
                ))
            } else {
                match std::env::set_current_dir(arg) {
                    Ok(()) => Some(format!(
                        "Working directory: {}",
                        std::env::current_dir().map(|p| p.display().to_string())
                            .unwrap_or_else(|_| arg.to_string())
                    )),
                    Err(e) => Some(format!("cd: {}", e)),
                }
            }
        }
        _ => None,
    }
}

// ── Text builders ─────────────────────────────────────────────────────────────

fn help_text() -> String {
    let mut s = String::new();
    let groups: &[(&str, &[(&str, &str)])] = &[
        ("session", &[
            ("/help",                     "show this help"),
            ("/config",                   "provider, model, URL"),
            ("/cost",                     "token usage and estimated cost"),
            ("/history",                  "message count"),
            ("/clear",                    "clear conversation history"),
            ("/compact",                  "summarize and compress history"),
            ("/sessions [N]",             "browse and resume old sessions"),
            ("/cd <path>",                "change working directory"),
            ("/exit",                     "quit"),
        ]),
        ("model & provider", &[
            ("/model <id>",               "switch model for this session"),
            ("/models",                   "list models on server"),
            ("/provider",                 "switch provider interactively"),
            ("/permissions ask|auto|deny","change permission mode"),
        ]),
        ("code", &[
            ("/tasks",                    "browse & execute task sessions"),
            ("/index [path|stats]",       "reindex AST code symbols"),
            ("/undo [file]",              "undo last file edit"),
            ("/init",                     "create CLAUDE.md for this project"),
            ("/run <workflow>",           "run a workflow"),
        ]),
        ("memory & skills", &[
            ("/memory list",              "list memory entries"),
            ("/memory get <key>",         "read a memory entry"),
            ("/memory set <k> <v>",       "write a memory entry"),
            ("/skill list",              "list available skills"),
            ("/audit [N]",               "show last N audit log lines"),
            ("/hooks",                   "list configured hooks"),
            ("/mcp [list|edit|path]",    "view/edit MCP server configs"),
        ]),
        ("git", &[
            ("/branch <name>",           "create a new branch"),
            ("/branches",                "list branches"),
            ("/switch <name>",           "switch branch"),
            ("/merge <name>",            "merge branch"),
        ]),
    ];

    s.push_str("zap slash commands\n");
    s.push_str(&"─".repeat(54));
    for (group, cmds) in groups {
        s.push_str(&format!("\n{}\n", group));
        for (cmd, desc) in *cmds {
            s.push_str(&format!("  {:<32} {}\n", cmd, desc));
        }
    }
    s.push_str(&"─".repeat(54));
    s.push_str("\ntip: type /commands to open the picker");
    s
}

fn config_text(session: &Session) -> String {
    let provider = match session.config.provider {
        Provider::Anthropic => "Anthropic API".to_string(),
        Provider::OpenAi    => session.base_url.as_deref()
            .unwrap_or("OpenAI-compatible")
            .to_string(),
    };
    let mode = match session.permissions.mode {
        PermissionMode::Ask  => "ask",
        PermissionMode::Auto => "auto",
        PermissionMode::Deny => "deny",
    };
    let url = session.base_url.as_deref()
        .unwrap_or("https://api.anthropic.com");
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "?".to_string());

    let mut s = String::new();
    s.push_str("configuration\n");
    s.push_str(&"─".repeat(44));
    s.push('\n');
    let kv = |k: &str, v: &str| format!("  {:<20} {}\n", k, v);
    s.push_str(&kv("provider",    &provider));
    s.push_str(&kv("model",       &session.model));
    s.push_str(&kv("base_url",    url));
    s.push_str(&kv("permissions", mode));
    s.push_str(&kv("turns",       &session.turn_count.to_string()));
    s.push_str(&kv("directory",   &cwd));
    s.push_str(&"─".repeat(44));
    s
}

fn cost_text(session: &Session) -> String {
    let (cost_in, cost_out) = crate::ui::cost_per_million(&session.model);
    let u = &session.session_usage;
    let mut s = String::new();
    s.push_str("Session token usage\n");
    s.push_str(&"─".repeat(44));
    s.push('\n');
    s.push_str(&format!("  {:<18} {}\n", "input",  u.input_tokens));
    s.push_str(&format!("  {:<18} {}\n", "output", u.output_tokens));
    if u.cache_read_tokens > 0 {
        s.push_str(&format!("  {:<18} {}\n", "cache read",  u.cache_read_tokens));
        s.push_str(&format!("  {:<18} {}\n", "cache write", u.cache_write_tokens));
    }
    if cost_in > 0.0 {
        let total = (u.input_tokens  as f64 * cost_in
                   + u.output_tokens as f64 * cost_out) / 1_000_000.0;
        s.push_str(&format!("  {:<18} ${:.4}\n", "est. cost", total));
    }
    s.push_str(&"─".repeat(44));
    s
}
