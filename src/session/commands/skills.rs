use std::sync::atomic::Ordering;
use colored::Colorize;
use crate::llm_client::{BeforeOutput, ContentBlock, Message};
use super::super::Session;

/// Validates a skill name: alphanumeric, hyphens, underscores only.
/// Rejects path traversal characters (`/`, `\`, `.`) that could escape the skills dir.
fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

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
                if !is_valid_skill_name(name) {
                    println!("  {} Invalid skill name. Use letters, digits, hyphens, underscores only.", "✗".red());
                    return;
                }
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
                if !is_valid_skill_name(skill_name) {
                    println!("  {} Invalid skill name. Use letters, digits, hyphens, underscores only.", "✗".red());
                    return;
                }
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
                    let turn_label = format!("#{:<3}", turn).truecolor(100, 95, 130);
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
                            why.truecolor(255, 140, 80).to_string()
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
    if !domain_scope.is_empty() {
        let names: Vec<&str> = domain_scope.iter().map(String::as_str).collect();
        s.push_str(&format!("Scope: {}  ·  /skill scope reset to clear\n", names.join(", ")));
    } else {
        s.push_str("All domain skills active  ·  /skill scope add <name> to restrict\n");
    }
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
