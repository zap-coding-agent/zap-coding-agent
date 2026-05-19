/// Task-oriented session startup.
///
/// Flow:
///   pick_session_mode()           → Vibe | Task
///   run_task_planning(…)          → clarifying Qs → LLM plan → skill matching → tasks.md
///
/// All LLM calls accept a `&dyn LlmProvider` so this module has no dependency
/// on Session — the caller (agent_core) passes only the pieces it needs.
use anyhow::Result;
use colored::Colorize;

use crate::{
    llm_client::{ContentBlock, LlmProvider, Message},
    skill_manager::{Skill, SkillSource},
};

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SessionMode {
    Vibe,
    Task,
}

#[derive(Debug, Clone)]
pub struct PlannedTask {
    pub title:      String,
    pub steps:      Vec<String>,
    pub skill_name: Option<String>,  // None = no skill matched / created yet
    pub verify:     String,
}

#[derive(Debug, Clone)]
pub struct TaskPlan {
    pub goal:        String,
    pub folder_name: String,   // kebab-case slug used for .zap/tasks/<folder>/
    pub tasks:       Vec<PlannedTask>,
}

// ── Mode picker ───────────────────────────────────────────────────────────────

pub fn pick_session_mode() -> SessionMode {
    use inquire::{
        ui::{Attributes, Color, RenderConfig, StyleSheet},
        Select,
    };

    let options = vec![
        "Vibe  — start talking, no structure (default)",
        "Task  — plan first, create tasks.md, skill-driven steps",
    ];

    let cfg = RenderConfig::default()
        .with_prompt_prefix(
            inquire::ui::Styled::new("  ◆").with_fg(Color::LightYellow),
        )
        .with_highlighted_option_prefix(
            inquire::ui::Styled::new(" ❯").with_fg(Color::LightYellow),
        )
        .with_selected_option(Some(
            StyleSheet::new()
                .with_fg(Color::LightCyan)
                .with_attr(Attributes::BOLD),
        ))
        .with_help_message(StyleSheet::new().with_fg(Color::DarkGrey));

    match Select::new("How do you want to work?", options)
        .with_render_config(cfg)
        .with_help_message("↑↓ select   Enter confirm   Esc = Vibe")
        .with_page_size(2)
        .prompt_skippable()
    {
        Ok(Some(s)) if s.starts_with("Task") => SessionMode::Task,
        _ => SessionMode::Vibe,
    }
}

// ── Main planning flow ────────────────────────────────────────────────────────

/// Full task-planning flow. Returns `None` if the user aborts.
pub async fn run_task_planning(
    client: &dyn LlmProvider,
    model:  &str,
    skills: &[Skill],
) -> Result<Option<TaskPlan>> {
    println!();
    println!(
        "  {} {} {}",
        "◆".truecolor(255, 210, 50),
        "Task mode".truecolor(255, 210, 50).bold(),
        "— plan first, then build".truecolor(100, 95, 130),
    );
    println!();

    // ── Step 1: get the goal ─────────────────────────────────────────────────
    let goal = prompt_line("  What do you want to build or fix?  ")?;
    if goal.trim().is_empty() {
        println!("  {} No goal entered — switching to Vibe mode.", "·".dimmed());
        return Ok(None);
    }
    let goal = goal.trim().to_string();

    // ── Step 2: LLM generates clarifying questions ───────────────────────────
    println!();
    println!("  {} Generating clarifying questions…", "◌".dimmed());
    let questions = fetch_clarifying_questions(client, &goal).await?;

    // ── Step 3: ask questions and collect answers ─────────────────────────────
    let mut qa_pairs: Vec<(String, String)> = Vec::new();
    if !questions.is_empty() {
        println!();
        for (i, q) in questions.iter().enumerate() {
            let label = format!("  Q{} {}", i + 1, q);
            let answer = prompt_line(&format!("{}\n  › ", label.truecolor(150, 140, 170)))?;
            if !answer.trim().is_empty() {
                qa_pairs.push((q.clone(), answer.trim().to_string()));
            }
        }
    }

    // ── Step 4: LLM generates the structured plan ────────────────────────────
    println!();
    println!("  {} Building implementation plan…", "◌".dimmed());
    let mut plan = fetch_task_plan(client, &goal, &qa_pairs, skills).await?;

    // ── Step 5: resolve missing skills (generates real content via LLM) ─────
    resolve_missing_skills(client, &mut plan, skills).await?;

    // ── Step 6: write tasks.md ───────────────────────────────────────────────
    let path = write_tasks_md(&plan, skills)?;

    println!();
    println!("  {} Task plan ready", "✓".green().bold());
    println!("  {}", "┌─────────────────────────────────────────────────────────".truecolor(60,55,80));
    println!("  {}  {}", "│".truecolor(60,55,80), path.display().to_string().cyan().bold());
    println!("  {}  {} task(s)  ·  skills: {}",
        "│".truecolor(60,55,80),
        plan.tasks.len().to_string().cyan(),
        plan.tasks.iter()
            .filter_map(|t| t.skill_name.as_deref())
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ")
            .truecolor(100,95,130),
    );
    println!("  {}", "└─────────────────────────────────────────────────────────".truecolor(60,55,80));
    println!("  {} Use {} to navigate, pick, and execute tasks",
        "◌".dimmed(), "/tasks".cyan().bold());
    println!();

    // ── Step 7: print task summary ───────────────────────────────────────────
    print_plan_summary(&plan);

    // Suppress unused warning.
    let _ = model;

    Ok(Some(plan))
}

// ── LLM calls ─────────────────────────────────────────────────────────────────

async fn fetch_clarifying_questions(
    client: &dyn LlmProvider,
    goal: &str,
) -> Result<Vec<String>> {
    let prompt = format!(
        "The user wants to: \"{goal}\"\n\n\
         Generate 2-3 SHORT clarifying questions that would significantly improve \
         an implementation plan. Focus on: tech stack, constraints, scope, or \
         existing code context. Return ONLY a JSON array of strings, no prose.\n\
         Example: [\"What language/framework?\", \"Is this a new feature or a fix?\"]"
    );

    let response = client
        .send(
            "You are a concise software planning assistant. Return only valid JSON.",
            &[Message::user_text(&prompt)],
            &[],
            None, 0,
        )
        .await?;

    let text = extract_text(&response.content);
    parse_string_array(&text).or_else(|_| Ok(Vec::new()))
}

async fn fetch_task_plan(
    client: &dyn LlmProvider,
    goal: &str,
    qa_pairs: &[(String, String)],
    skills: &[Skill],
) -> Result<TaskPlan> {
    let skill_names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    let skill_list = skill_names.join(", ");

    let qa_block = if qa_pairs.is_empty() {
        String::new()
    } else {
        let lines: Vec<String> = qa_pairs
            .iter()
            .map(|(q, a)| format!("Q: {q}\nA: {a}"))
            .collect();
        format!("\nContext from clarifying questions:\n{}", lines.join("\n"))
    };

    let prompt = format!(
        "Goal: \"{goal}\"{qa_block}\n\n\
         Available skills (knowledge modules already in zap): {skill_list}\n\n\
         Create a concise implementation plan. Return ONLY this JSON (no prose, no code fences):\n\
         {{\n\
           \"folder_name\": \"kebab-case-slug-max-5-words\",\n\
           \"tasks\": [\n\
             {{\n\
               \"title\": \"Short task title\",\n\
               \"steps\": [\"Specific step 1\", \"Specific step 2\"],\n\
               \"suggested_skill\": \"rust\",\n\
               \"verify\": \"How to verify this task is done\"\n\
             }}\n\
           ]\n\
         }}\n\n\
         Rules:\n\
         - 3 to 7 tasks\n\
         - suggested_skill must be one of the available skill names above, or null if none fits\n\
         - steps must be specific and actionable\n\
         - folder_name: lowercase, hyphens only, max 5 words"
    );

    let response = client
        .send(
            "You are a precise software planning assistant. Return only valid JSON.",
            &[Message::user_text(&prompt)],
            &[],
            None, 0,
        )
        .await?;

    let text = extract_text(&response.content);
    parse_task_plan(&text, goal)
}

// ── Missing skill resolution ──────────────────────────────────────────────────

async fn resolve_missing_skills(
    client:          &dyn LlmProvider,
    plan:            &mut TaskPlan,
    existing_skills: &[Skill],
) -> Result<()> {
    let tasks_needing_skill: Vec<usize> = plan
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.skill_name.is_none())
        .map(|(i, _)| i)
        .collect();

    if tasks_needing_skill.is_empty() {
        return Ok(());
    }

    println!();
    println!(
        "  {} {} task(s) have no matching built-in skill:",
        "⚠".yellow(),
        tasks_needing_skill.len()
    );

    for &i in &tasks_needing_skill {
        let task_title = plan.tasks[i].title.clone();
        let task_steps = plan.tasks[i].steps.clone();

        println!("    {} {}", "·".dimmed(), task_title.cyan());

        let input = prompt_line(
            "  Skill name to create (Enter to skip, or type a name — content generated by LLM): ",
        )?;
        let skill_name = input.trim().to_string();

        if skill_name.is_empty() {
            continue;
        }

        let already_exists = existing_skills.iter().any(|s| s.name == skill_name);
        if already_exists {
            println!("  {} skill '{}' already exists — linking task to it", "·".dimmed(), skill_name.cyan());
        } else {
            println!(
                "  {} Generating skill content for '{}'…",
                "◌".dimmed(),
                skill_name.cyan()
            );
            let content = generate_skill_content(client, &skill_name, &task_title, &task_steps).await
                .unwrap_or_else(|_| default_skill_template(&skill_name, &task_title));

            write_skill_file(&skill_name, &content)?;
            println!(
                "  {} skill created → .zap/skills/{}.md",
                "✓".green(),
                skill_name.cyan()
            );
        }

        plan.tasks[i].skill_name = Some(skill_name);
    }

    Ok(())
}

/// Ask the LLM to write a real, opinionated skill file for `skill_name`.
async fn generate_skill_content(
    client:     &dyn LlmProvider,
    skill_name: &str,
    task_title: &str,
    task_steps: &[String],
) -> Result<String> {
    let steps_block = task_steps
        .iter()
        .map(|s| format!("- {s}"))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Generate a concise, practical skill file for: \"{skill_name}\"\n\
         Context — this skill is needed for the task: \"{task_title}\"\n\
         Task steps:\n{steps_block}\n\n\
         Return ONLY a complete skill markdown file in this exact format \
         (no explanation, no code fences):\n\
         ---\n\
         name: {skill_name}\n\
         description: One sentence describing what this skill covers.\n\
         trigger: [\"keyword1\", \"keyword2\", \"keyword3\"]\n\
         tokens: ~400\n\
         ---\n\
         \n\
         ## {skill_name} guidelines\n\
         \n\
         [3-6 specific, actionable guidelines. No fluff. No empty placeholders.]"
    );

    let response = client
        .send(
            "You write concise, expert skill files for AI coding agents. Return only the skill markdown.",
            &[Message::user_text(&prompt)],
            &[],
            None, 0,
        )
        .await?;

    Ok(extract_text(&response.content))
}

/// Fallback if the LLM call fails — still better than an empty template.
fn default_skill_template(name: &str, task_title: &str) -> String {
    format!(
        "---\n\
         name: {name}\n\
         description: Guidelines for {task_title}.\n\
         trigger: [\"{name}\"]\n\
         tokens: ~300\n\
         ---\n\
         \n\
         ## {name} guidelines\n\
         \n\
         <!-- LLM generation failed. Fill in guidelines for: {task_title} -->\n\
         \n\
         1. Follow established conventions for this domain.\n\
         2. Keep changes minimal and targeted.\n\
         3. Verify correctness before marking the task done.\n"
    )
}

fn write_skill_file(name: &str, content: &str) -> Result<()> {
    let dir = std::path::PathBuf::from(".zap").join("skills");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.md"));
    std::fs::write(path, content)?;
    Ok(())
}

// ── TaskFile — read & parse an existing tasks.md ─────────────────────────────

#[derive(Debug, Clone)]
pub struct TaskItem {
    pub number:     usize,
    pub title:      String,
    pub skill_name: Option<String>,
    pub steps:      Vec<(bool, String)>,  // (checked, text)
    pub verify:     String,
}

impl TaskItem {
    pub fn is_done(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|(checked, _)| *checked)
    }

    /// Build the prompt sent to the agent when the user selects this task.
    pub fn execution_prompt(&self) -> String {
        let skill_line = self.skill_name
            .as_deref()
            .map(|s| format!("Skill: {s}\n\n"))
            .unwrap_or_default();

        let steps: Vec<String> = self.steps.iter().map(|(checked, text)| {
            let marker = if *checked { "[x]" } else { "[ ]" };
            format!("- {marker} {text}")
        }).collect();

        format!(
            "[Task {}: {}]\n\n{}{}\n\nVerify: {}\n\nPlease complete the unchecked steps.",
            self.number, self.title,
            skill_line,
            steps.join("\n"),
            self.verify,
        )
    }
}

#[derive(Debug, Clone)]
pub struct TaskFile {
    pub goal:        String,
    pub folder:      String,
    pub path:        std::path::PathBuf,
    pub tasks:       Vec<TaskItem>,
}

impl TaskFile {
    pub fn done_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.is_done()).count()
    }
}

/// Scan `.zap/tasks/` and return all parseable TaskFiles.
pub fn discover_task_files() -> Vec<TaskFile> {
    let base = std::path::PathBuf::from(".zap").join("tasks");
    let mut files = Vec::new();

    let Ok(entries) = std::fs::read_dir(&base) else { return files };

    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() { continue; }
        let tasks_path = dir.join("tasks.md");
        if !tasks_path.exists() { continue; }
        let folder = dir.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();
        if let Ok(tf) = parse_tasks_md(&tasks_path, &folder) {
            files.push(tf);
        }
    }

    files.sort_by(|a, b| a.folder.cmp(&b.folder));
    files
}

fn parse_tasks_md(path: &std::path::Path, folder: &str) -> Result<TaskFile> {
    let raw = std::fs::read_to_string(path)?;
    let mut goal      = String::new();
    let mut tasks     = Vec::new();

    // Parser state
    let mut cur_number:     usize            = 0;
    let mut cur_title:      String           = String::new();
    let mut cur_skill:      Option<String>   = None;
    let mut cur_steps:      Vec<(bool,String)> = Vec::new();
    let mut cur_verify:     String           = String::new();
    let mut in_task = false;

    let flush = |number, title: &str, skill: &Option<String>,
                  steps: &[(bool,String)], verify: &str,
                  tasks: &mut Vec<TaskItem>| {
        if number > 0 {
            tasks.push(TaskItem {
                number,
                title:      title.to_string(),
                skill_name: skill.clone(),
                steps:      steps.to_vec(),
                verify:     verify.to_string(),
            });
        }
    };

    for line in raw.lines() {
        if line.starts_with("# ") && goal.is_empty() {
            goal = line.trim_start_matches("# ").to_string();
            continue;
        }

        // New task section: ## Task N — Title
        if line.starts_with("## Task ") {
            flush(cur_number, &cur_title, &cur_skill, &cur_steps, &cur_verify, &mut tasks);
            // Reset state
            let rest = line.trim_start_matches("## Task ");
            let (num_str, title_rest) = rest.split_once(" — ").unwrap_or((rest, ""));
            cur_number = num_str.trim().parse().unwrap_or(tasks.len() + 1);
            cur_title  = title_rest.to_string();
            cur_skill  = None;
            cur_steps  = Vec::new();
            cur_verify = String::new();
            in_task    = true;
            continue;
        }

        if !in_task { continue; }

        // Skill line
        if line.starts_with("Skill: `") {
            if let Some(inner) = line.strip_prefix("Skill: `") {
                let name = inner.split('`').next().unwrap_or("").to_string();
                if !name.is_empty() && name != "*(none" {
                    cur_skill = Some(name);
                }
            }
            continue;
        }

        // Step lines
        if line.starts_with("- [ ] ") {
            cur_steps.push((false, line.trim_start_matches("- [ ] ").to_string()));
            continue;
        }
        if line.starts_with("- [x] ") || line.starts_with("- [X] ") {
            cur_steps.push((true, line[6..].to_string()));
            continue;
        }

        // Verify line
        if line.starts_with("Verify: ") {
            cur_verify = line.trim_start_matches("Verify: ").to_string();
        }
    }

    // Flush last task
    flush(cur_number, &cur_title, &cur_skill, &cur_steps, &cur_verify, &mut tasks);

    Ok(TaskFile {
        goal,
        folder: folder.to_string(),
        path:   path.to_path_buf(),
        tasks,
    })
}

// ── tasks.md writer ───────────────────────────────────────────────────────────

pub fn write_tasks_md(plan: &TaskPlan, skills: &[Skill]) -> Result<std::path::PathBuf> {
    let dir = std::path::PathBuf::from(".zap")
        .join("tasks")
        .join(&plan.folder_name);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("tasks.md");

    let today = chrono_today();
    let skill_names_used: Vec<&str> = plan
        .tasks
        .iter()
        .filter_map(|t| t.skill_name.as_deref())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mut lines: Vec<String> = Vec::new();
    lines.push(format!("# {}", plan.goal));
    lines.push(format!("Created: {today}"));
    if !skill_names_used.is_empty() {
        lines.push(format!("Skills: {}", skill_names_used.join(", ")));
    }
    lines.push(String::new());
    lines.push("---".into());
    lines.push(String::new());

    for (i, task) in plan.tasks.iter().enumerate() {
        lines.push(format!("## Task {} — {}", i + 1, task.title));
        lines.push(String::new());

        if let Some(ref sname) = task.skill_name {
            let desc = skills
                .iter()
                .find(|s| &s.name == sname)
                .and_then(|s| {
                    if s.description.is_empty() { None } else { Some(s.description.as_str()) }
                })
                .unwrap_or("");
            let source = skills
                .iter()
                .find(|s| &s.name == sname)
                .map(|s| match s.source {
                    SkillSource::Project     => " [project]",
                    SkillSource::Global      => " [global]",
                    SkillSource::Bundled     => " [built-in]",
                    SkillSource::External(_) => " [external]",
                })
                .unwrap_or(" [stub]");

            if desc.is_empty() {
                lines.push(format!("Skill: `{sname}`{source}"));
            } else {
                lines.push(format!("Skill: `{sname}`{source} — {desc}"));
            }
        } else {
            lines.push("Skill: *(none — consider creating one)*".into());
        }

        lines.push(String::new());
        for step in &task.steps {
            lines.push(format!("- [ ] {step}"));
        }
        lines.push(String::new());
        lines.push(format!("Verify: {}", task.verify));
        lines.push(String::new());
    }

    lines.push("---".into());
    lines.push(String::new());
    lines.push("*Generated by zap · `/skill show <name>` to inspect any skill*".into());

    std::fs::write(&path, lines.join("\n"))?;
    Ok(path)
}

// ── Plan summary (terminal) ───────────────────────────────────────────────────

fn print_plan_summary(plan: &TaskPlan) {
    println!(
        "  {} {}",
        "◆".truecolor(255, 210, 50),
        plan.goal.truecolor(255, 210, 50).bold()
    );
    println!("  {}", "─".repeat(50).truecolor(60, 55, 80));
    for (i, task) in plan.tasks.iter().enumerate() {
        let skill_tag = task
            .skill_name
            .as_deref()
            .map(|s| format!("  [{}]", s))
            .unwrap_or_default();
        println!(
            "  {}  {}{}",
            format!("{}.", i + 1).truecolor(100, 95, 130),
            task.title.cyan(),
            skill_tag.truecolor(100, 95, 130),
        );
    }
    println!("  {}", "─".repeat(50).truecolor(60, 55, 80));
    println!();
}

// ── Parsing helpers ───────────────────────────────────────────────────────────

fn extract_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|b| {
            if let ContentBlock::Text { text } = b {
                Some(text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

fn parse_string_array(text: &str) -> Result<Vec<String>> {
    // Strip markdown code fences if present.
    let clean = strip_fences(text);
    let arr: Vec<String> = serde_json::from_str(&clean)?;
    Ok(arr)
}

fn parse_task_plan(text: &str, goal: &str) -> Result<TaskPlan> {
    let clean = strip_fences(text);
    let v: serde_json::Value = serde_json::from_str(&clean)?;

    let folder_name = v["folder_name"]
        .as_str()
        .map(sanitize_folder_name)
        .unwrap_or_else(|| sanitize_folder_name(goal));

    let tasks = v["tasks"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|t| PlannedTask {
                    title: t["title"].as_str().unwrap_or("Untitled task").to_string(),
                    steps: t["steps"]
                        .as_array()
                        .map(|s| {
                            s.iter()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect()
                        })
                        .unwrap_or_default(),
                    skill_name: t["suggested_skill"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(str::to_string),
                    verify: t["verify"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(TaskPlan {
        goal: goal.to_string(),
        folder_name,
        tasks,
    })
}

fn strip_fences(text: &str) -> String {
    let trimmed = text.trim();
    // Remove ```json ... ``` or ``` ... ``` wrappers.
    if let Some(inner) = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
    {
        if let Some(body) = inner.strip_suffix("```") {
            return body.trim().to_string();
        }
    }
    trimmed.to_string()
}

pub fn sanitize_folder_name(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-")
}

// ── I/O helpers ───────────────────────────────────────────────────────────────

fn prompt_line(label: &str) -> Result<String> {
    use std::io::Write;
    print!("{label}");
    std::io::stdout().flush()?;
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;
    Ok(buf.trim_end_matches('\n').trim_end_matches('\r').to_string())
}

fn chrono_today() -> String {
    // Use file mtime as a proxy for "today" — avoids pulling in chrono.
    // Falls back to a static placeholder if metadata is unavailable.
    std::fs::metadata(".")
        .and_then(|m| m.modified())
        .map(|t| {
            let secs = t
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            // Simple YYYY-MM-DD from Unix timestamp (good enough for a header).
            let days  = secs / 86400;
            let years = 1970 + days / 365;
            let rem   = days % 365;
            let month = rem / 30 + 1;
            let day   = rem % 30 + 1;
            format!("{years}-{month:02}-{day:02}")
        })
        .unwrap_or_else(|_| "today".to_string())
}
