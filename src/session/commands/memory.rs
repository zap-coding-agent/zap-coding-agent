use colored::Colorize;
use super::super::Session;

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

    pub fn cmd_mcp(&self, arg: &str) {
        let global_path = dirs::home_dir()
            .map(|h| h.join(".zap").join("mcp.json"));
        let project_path = std::path::PathBuf::from(".mcp.json");

        match arg.trim() {
            "" | "list" => {
                let global_cfg = global_path.as_ref()
                    .filter(|p| p.exists())
                    .map(|p| crate::mcp::load_file(p))
                    .unwrap_or_default();

                let project_cfg = if project_path.exists() {
                    crate::mcp::load_file(&project_path)
                } else {
                    crate::mcp::McpConfig::default()
                };

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
                        println!("    {} {} [{}]  {}",
                            "◆".truecolor(255, 210, 50),
                            name.truecolor(100, 210, 255).bold(),
                            status,
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

            "edit" | "edit global" => {
                let path = match global_path {
                    Some(ref p) => p.clone(),
                    None => { println!("  {} could not determine home dir", "✗".red()); return; }
                };
                if !path.exists() {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent).ok();
                    }
                    std::fs::write(&path,
                        "{\n  \"mcpServers\": {\n  }\n}\n"
                    ).ok();
                    println!("  {} created {}", "✓".green(), path.display().to_string().cyan());
                }
                open_in_editor(&path);
            }

            "edit project" => {
                if !project_path.exists() {
                    std::fs::write(&project_path,
                        "{\n  \"mcpServers\": {\n  }\n}\n"
                    ).ok();
                    println!("  {} created {}", "✓".green(), project_path.display().to_string().cyan());
                }
                open_in_editor(&project_path);
            }

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
