use crate::config::{PermissionMode, Provider};
use crate::session::Session;

pub(super) fn help_text() -> String {
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
            ("/goal <condition>",         "run autonomously until condition met (max 20 turns)"),
            ("/goal stop",               "cancel an active goal"),
            ("/tasks",                    "browse & execute task sessions"),
            ("/index [path|stats]",       "reindex AST code symbols"),
            ("/index quality",            "code quality: god objects, coupling, dead code, score"),
            ("/deploy [--check]",         "build & install zap with live output, no timeout"),
            ("/undo [file]",              "undo last file edit"),
            ("/init",                     "set up project (ZAP.md, index, project.json)"),
            ("/run <workflow>",           "run a workflow"),
            ("/diff",                     "show git diff in TUI viewer"),
            ("/attach <path>",            "stage an image for the next message"),
            ("/paste",                    "paste image from clipboard"),
        ]),
        ("memory & skills", &[
            ("/memory list",              "list memory entries"),
            ("/memory get <key>",         "read a memory entry"),
            ("/memory set <k> <v>",       "write a memory entry"),
            ("/skill list",              "list all skills (built-in, global, project)"),
            ("/skill use <name>",        "pin a skill — injected every turn"),
            ("/skill unuse <name>",      "unpin a skill"),
            ("/skill show <name>",       "preview a skill's content"),
            ("/skill log",               "show which skills fired (or didn't) per turn"),
            ("/audit [N]",               "show last N audit log lines"),
            ("/hooks",                   "list configured hooks"),
            ("/mcp [list|edit|path]",    "view/edit MCP server configs"),
        ]),
        ("git & branches", &[
            ("/branch <name>",           "create a new branch"),
            ("/branches",                "list branches"),
            ("/switch <name>",           "switch branch"),
            ("/merge <name>",            "merge branch"),
        ]),
        ("remote", &[
            ("/remote [port]",           "start remote control server + tunnel"),
            ("/remote stop",             "stop remote control"),
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

pub(super) fn config_text(session: &Session) -> String {
    let provider = match session.config.provider {
        Provider::Anthropic => "Anthropic API".to_string(),
        Provider::OpenAi    => {
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

pub(super) fn cost_text(session: &Session) -> String {
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

pub(super) fn index_quality_text() -> String {
    use std::fmt::Write;

    let report = match crate::code_index::global_quality_report() {
        Some(r) => r,
        None => return "Code index not loaded. Run /index first.".to_string(),
    };
    let file_stats = crate::code_index::global_file_line_counts();

    let score = report.score();
    let score_icon = if score >= 80 { "✓" } else if score >= 60 { "⚡" } else { "⚠" };

    let mut s = String::new();
    let _ = writeln!(s, "Code Health  ·  {} files  ·  {} symbols  ·  {} {}/100",
        report.total_files, report.total_syms, score_icon, score);
    let _ = writeln!(s, "{}", "─".repeat(60));

    if !file_stats.is_empty() {
        let max_lines = file_stats.iter().map(|(_, _, l)| *l).max().unwrap_or(1);
        let _ = writeln!(s, "\nFile sizes  (lines)");
        let _ = writeln!(s, "{}", "─".repeat(60));
        for (path, sym_count, line_count) in &file_stats {
            let icon = if *line_count > 1000 { "⚠" } else if *line_count > 500 { "⚡" } else { "·" };
            let bar_len = (line_count * 22 / max_lines).max(1);
            let bar: String = "█".repeat(bar_len);
            let _ = writeln!(s, "  {} {:>4}  {:<38} {}  {} sym",
                icon, line_count, path, bar, sym_count);
        }
        let _ = writeln!(s, "\n  ⚠ >1000 lines  ⚡ 500-1000  · healthy");
    }

    if !report.god_objects.is_empty() {
        let _ = writeln!(s, "\nGod objects  (>15 methods — split candidates)");
        let _ = writeln!(s, "{}", "─".repeat(60));
        for (label, methods, path) in &report.god_objects {
            let short = path.rsplit('/').next().unwrap_or(path);
            let _ = writeln!(s, "  {:<30} {:>3} methods  ({})", label, methods, short);
        }
    }

    if !report.high_coupling.is_empty() {
        let _ = writeln!(s, "\nHigh coupling  (called from many places)");
        let _ = writeln!(s, "{}", "─".repeat(60));
        for (name, path, line, refs) in &report.high_coupling {
            let short = path.rsplit('/').next().unwrap_or(path);
            let _ = writeln!(s, "  {:<32} {:>2} refs  ({}:{})", name, refs, short, line);
        }
    }

    if !report.complex_fns.is_empty() {
        let _ = writeln!(s, "\nComplex functions  (long signatures — consider breaking up)");
        let _ = writeln!(s, "{}", "─".repeat(60));
        for (name, path, line) in &report.complex_fns {
            let short = path.rsplit('/').next().unwrap_or(path);
            let _ = writeln!(s, "  {}  ({}:{})", name, short, line);
        }
    }

    if !report.dead_candidates.is_empty() {
        let _ = writeln!(s, "\nDead code candidates  (pub fn, ≤1 reference)");
        let _ = writeln!(s, "{}", "─".repeat(60));
        for (name, path, line) in &report.dead_candidates {
            let short = path.rsplit('/').next().unwrap_or(path);
            let _ = writeln!(s, "  {:<32} ({}:{})", name, short, line);
        }
    }

    s
}

pub(super) fn handle_skill_inline(session: &mut Session, arg: &str) -> String {
    let parts: Vec<&str> = arg.splitn(2, ' ').collect();
    let subcmd = parts.first().copied().unwrap_or("list");
    let name   = parts.get(1).copied().unwrap_or("").trim();

    match subcmd {
        "" | "list" => {
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
