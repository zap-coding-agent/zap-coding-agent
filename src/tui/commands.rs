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
    ("/new",               "start a fresh session (clear history)"),
    ("/sessions",          "browse and resume old sessions"),
    ("/cd",                "change working directory"),
    ("/model",             "switch model for this session"),
    ("/models",            "list available models"),
    ("/provider",          "switch provider interactively"),
    ("/permissions",       "change permission mode"),
    ("/tasks",             "browse & execute task sessions"),
    ("/think",             "toggle extended thinking (on/off/N tokens)"),
    ("/goal",              "run autonomously until condition met"),
    ("/index",             "reindex AST code symbols"),
    ("/undo",              "undo last file edit"),
    ("/init",              "set up project (ZAP.md, index, project.json)"),
    ("/run",               "run a workflow"),
    ("/memory",            "manage memory entries"),
    ("/skill",             "list, use, show skills"),
    ("/audit",             "show last N audit log lines"),
    ("/hooks",             "list configured hooks"),
    ("/mcp",               "view/edit MCP server configs"),
    ("/remote",            "start remote control — get a URL to code from anywhere"),
    ("/branch",            "create a conversation branch"),
    ("/branches",          "list conversation branches"),
    ("/switch",            "switch conversation branch"),
    ("/merge",             "merge a conversation branch"),
    ("/exit",              "quit zap"),
];

/// Return commands matching `input` (case-insensitive).
///
/// - `/skill <tab>` → `/skill list|use|unuse|show|scope` sub-commands
/// - `/skill use <tab>` etc. → matching skill names from `skill_names`
/// - otherwise → SLASH_COMMANDS filtered by prefix
pub fn filter_commands(input: &str, skill_names: &[String]) -> Vec<(String, String)> {
    let lower = input.to_lowercase();

    // /skill use|show|unuse <name> → dynamic skill-name completions
    for prefix in &["/skill use ", "/skill show ", "/skill unuse "] {
        if lower.starts_with(prefix) {
            let typed = &lower[prefix.len()..];
            return skill_names
                .iter()
                .filter(|n| n.to_lowercase().starts_with(typed))
                .map(|n| (format!("{}{}", prefix, n), format!("skill: {}", n)))
                .collect();
        }
    }

    // /skill<space> → sub-command completions
    if lower.starts_with("/skill ") {
        let typed = &lower[7..]; // after "/skill "
        return [
            ("list",  "list all skills (built-in, global, project)"),
            ("use",   "pin skill — injected every turn"),
            ("unuse", "unpin a skill"),
            ("show",  "preview a skill's content"),
            ("scope", "manage domain scope"),
        ]
        .iter()
        .filter(|(sc, _)| sc.starts_with(typed))
        .map(|(sc, desc)| (format!("/skill {}", sc), desc.to_string()))
        .collect();
    }

    // Default: filter SLASH_COMMANDS by prefix
    SLASH_COMMANDS
        .iter()
        .filter(|(cmd, _)| cmd.starts_with(lower.as_str()))
        .map(|(cmd, desc)| (cmd.to_string(), desc.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_top_level_commands_by_prefix() {
        let cmds = filter_commands("/he", &[]);
        assert!(cmds.iter().any(|(c, _)| c == "/help"));
        assert!(cmds.iter().all(|(c, _)| c.starts_with("/he")));
    }

    #[test]
    fn slash_alone_returns_all_commands() {
        let cmds = filter_commands("/", &[]);
        assert_eq!(cmds.len(), SLASH_COMMANDS.len());
    }

    #[test]
    fn skill_space_shows_subcommands() {
        let cmds = filter_commands("/skill ", &[]);
        let names: Vec<&str> = cmds.iter().map(|(c, _)| c.as_str()).collect();
        assert!(names.contains(&"/skill list"));
        assert!(names.contains(&"/skill use"));
        assert!(names.contains(&"/skill show"));
        assert!(names.contains(&"/skill unuse"));
        assert!(names.contains(&"/skill scope"));
    }

    #[test]
    fn skill_use_space_shows_skill_names() {
        let skills = vec!["rust-expert".to_string(), "python-helper".to_string()];
        let cmds = filter_commands("/skill use ", &skills);
        let names: Vec<&str> = cmds.iter().map(|(c, _)| c.as_str()).collect();
        assert!(names.contains(&"/skill use rust-expert"));
        assert!(names.contains(&"/skill use python-helper"));
    }

    #[test]
    fn skill_use_filters_by_typed_prefix() {
        let skills = vec!["rust-expert".to_string(), "python-helper".to_string()];
        let cmds = filter_commands("/skill use ru", &skills);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].0, "/skill use rust-expert");
    }

    #[test]
    fn skill_show_and_unuse_also_complete_names() {
        let skills = vec!["myskill".to_string()];
        assert_eq!(filter_commands("/skill show ", &skills)[0].0, "/skill show myskill");
        assert_eq!(filter_commands("/skill unuse ", &skills)[0].0, "/skill unuse myskill");
    }

    #[test]
    fn filter_commands_no_skill_names_empty_completions() {
        let cmds = filter_commands("/skill use ", &[]);
        assert!(cmds.is_empty());
    }

    #[test]
    fn skill_subcommand_prefix_filtering() {
        let cmds = filter_commands("/skill l", &[]);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].0, "/skill list");
    }
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
        "/new" => {
            session.messages.clear();
            session.turn_count = 0;
            if let Ok(id) = session.store.save_session("(new session)", &session.config.model) {
                session.session_id = id;
            }
            Some("New session started. History cleared.".to_string())
        }
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
        "/think" => {
            let budget = session.thinking_budget;
            match arg {
                "" | "status" => {
                    let status = if budget == 0 {
                        "off".to_string()
                    } else {
                        format!("{} token budget", budget)
                    };
                    Some(format!(
                        "Extended thinking: {}\nUsage: /think on|off|<tokens>  (Anthropic only; requires claude-3-7-sonnet+)",
                        status
                    ))
                }
                "off" | "0" => {
                    session.thinking_budget = 0;
                    Some("Extended thinking disabled.".to_string())
                }
                "on" => {
                    session.thinking_budget = 8000;
                    Some("Extended thinking enabled (8000 token budget). Requires claude-3-7-sonnet or newer.".to_string())
                }
                n => match n.parse::<u32>() {
                    Ok(0) => {
                        session.thinking_budget = 0;
                        Some("Extended thinking disabled.".to_string())
                    }
                    Ok(v) => {
                        session.thinking_budget = v;
                        Some(format!("Extended thinking enabled ({} token budget).", v))
                    }
                    Err(_) => Some("Usage: /think on|off|<budget_tokens>  e.g. /think 8000".to_string()),
                }
            }
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
        "/skill" => Some(handle_skill_inline(session, arg)),
        "/remote" => {
            // Run asynchronously but return a placeholder immediately;
            // the actual URL is printed via zap_warn! once the tunnel is up.
            let port: u16 = arg.parse().unwrap_or(0);
            tokio::spawn(async move {
                crate::remote_channel::activate();
                match crate::remote::start_server(port).await {
                    Ok(actual_port) => {
                        crate::zap_warn!("⚡ remote server listening on http://127.0.0.1:{}", actual_port);
                        match crate::remote::launch_tunnel(actual_port).await {
                            Ok(url) => {
                                crate::zap_warn!("🌐 remote URL: {}", url);
                                crate::zap_warn!("   Open on any device — type messages, get responses in real time.");
                            }
                            Err(e) => crate::zap_warn!("tunnel failed: {} — use http://127.0.0.1:{} on the same network", e, actual_port),
                        }
                    }
                    Err(e) => crate::zap_warn!("remote server failed: {}", e),
                }
            });
            Some("⚡ Starting remote server and tunnel… URL will appear in a moment.".to_string())
        }
        _ => None,
    }
}

fn handle_skill_inline(session: &mut Session, arg: &str) -> String {
    let parts: Vec<&str> = arg.splitn(2, ' ').collect();
    let subcmd = parts.first().copied().unwrap_or("list");
    let name   = parts.get(1).copied().unwrap_or("").trim();

    match subcmd {
        "" | "list" => {
            // Reload skills from disk so project skills appear immediately.
            session.skills = crate::skill_manager::load_all_skills(&session.config.skill_paths);
            crate::session::commands::skill_list_text(
                &session.skills,
                &session.domain_scope,
                &session.pinned_skills,
            )
        }
        "use" => {
            if name.is_empty() {
                return "Usage: /skill use <name>".to_string();
            }
            if session.skills.iter().any(|s| s.name == name) {
                session.pinned_skills.insert(name.to_string());
                format!("✓ '{}' pinned — injected every turn. Use /skill unuse {} to remove.", name, name)
            } else {
                format!("✗ Skill '{}' not found. Run /skill list to see available skills.", name)
            }
        }
        "unuse" | "drop" | "unpin" => {
            if name.is_empty() {
                return "Usage: /skill unuse <name>".to_string();
            }
            if session.pinned_skills.remove(name) {
                format!("✓ '{}' unpinned.", name)
            } else {
                format!("'{}' was not pinned.", name)
            }
        }
        "show" => {
            if name.is_empty() {
                return "Usage: /skill show <name>".to_string();
            }
            match crate::skill_manager::load_all_skills(&session.config.skill_paths)
                .into_iter()
                .find(|s| s.name == name)
            {
                Some(sk) => crate::session::commands::skill_show_text(&sk),
                None     => format!("✗ Skill '{}' not found.", name),
            }
        }
        "scope" => {
            let scope_arg = name.trim();
            if scope_arg.is_empty() {
                // Show current scope
                if session.domain_scope.is_empty() {
                    "Domain scope: unrestricted (all domain skills are trigger-matchable).\n\
                     Use /skill scope add <name> to restrict.".to_string()
                } else {
                    let names: Vec<&str> = session.domain_scope.iter().map(String::as_str).collect();
                    format!(
                        "Domain scope: {}\n/skill scope add <name>  ·  /skill scope remove <name>  ·  /skill scope reset",
                        names.join(", ")
                    )
                }
            } else if let Some(n) = scope_arg.strip_prefix("add ").map(str::trim) {
                if session.skills.iter().any(|s| s.name == n && s.category == crate::skill_manager::SkillCategory::Domain) {
                    session.domain_scope.insert(n.to_string());
                    format!("✓ '{}' added to domain scope.", n)
                } else {
                    format!("✗ No domain skill named '{}'.", n)
                }
            } else if let Some(n) = scope_arg.strip_prefix("remove ").map(str::trim) {
                if session.domain_scope.remove(n) {
                    format!("✓ '{}' removed from domain scope.", n)
                } else {
                    format!("'{}' was not in scope.", n)
                }
            } else if scope_arg == "reset" {
                session.domain_scope.clear();
                "✓ Domain scope cleared — all domain skills are now trigger-matchable.".to_string()
            } else {
                "Usage: /skill scope  |  scope add <name>  |  scope remove <name>  |  scope reset".to_string()
            }
        }
        _ => "Usage: /skill [list | use <name> | unuse <name> | show <name> | scope [...]]".to_string(),
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
            ("/new",                      "start a fresh session"),
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
            ("/think [on|off|N]",         "extended thinking (Anthropic only)"),
        ]),
        ("code", &[
            ("/tasks",                    "browse & execute task sessions"),
            ("/index [path|stats]",       "reindex AST code symbols"),
            ("/undo [file]",              "undo last file edit"),
            ("/init",                     "set up project (ZAP.md, index, project.json)"),
            ("/run <workflow>",           "run a workflow"),
        ]),
        ("memory & skills", &[
            ("/memory list",              "list memory entries"),
            ("/memory get <key>",         "read a memory entry"),
            ("/memory set <k> <v>",       "write a memory entry"),
            ("/skill list",              "list all skills (built-in, global, project)"),
            ("/skill use <name>",        "pin a skill — injected every turn"),
            ("/skill unuse <name>",      "unpin a skill"),
            ("/skill show <name>",       "preview a skill's content"),
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
        Provider::OpenAi    => {
            // Strip endpoint suffix for display; show just host/base path.
            let raw = session.base_url.as_deref().unwrap_or("OpenAI-compatible");
            let raw = raw.strip_suffix("/chat/completions").unwrap_or(raw);
            let raw = raw.strip_suffix("/v1").unwrap_or(raw);
            raw.to_string()
        }
    };
    let mode = match session.permissions.mode {
        PermissionMode::Ask  => "ask",
        PermissionMode::Auto => "auto",
        PermissionMode::Deny => "deny",
    };
    let url_raw = session.base_url.as_deref().unwrap_or("https://api.anthropic.com/v1/messages");
    let url_raw = url_raw.strip_suffix("/chat/completions").unwrap_or(url_raw);
    let url     = url_raw.strip_suffix("/v1").unwrap_or(url_raw);
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
