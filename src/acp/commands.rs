//! Slash commands in ACP mode.
//!
//! In the TUI, slash commands are handled by the front-end, not the agent
//! loop, in three flavours — and each needs a different route to an editor:
//!
//! 1. Inline commands that already return text (`tui::commands::handle_inline`)
//!    are reused as-is.
//! 2. CLI-style commands that `println!` (`Session::handle_slash`) are run
//!    with stdout captured — the caller does that on [`Next::Fallback`].
//! 3. Commands that open pickers or wizards in the TUI get argument-driven
//!    versions here, since an editor has no terminal to draw them in.

use crate::config::Config;
use crate::llm_client::ContentBlock;
use crate::session::Session;
use crate::tui::commands::{SECTION_PREFIX, SLASH_COMMANDS};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// What the worker should do after a command has been looked at.
pub enum Next {
    /// Fully handled; any output was already emitted.
    Done,
    /// Run this text as a normal prompt turn (e.g. `/init`'s analysis prompt).
    Turn(String),
    /// Run turns until the model signals completion or `max_turns` is reached.
    Goal { condition: String, max_turns: usize },
    /// Not handled here — run `Session::handle_slash` with stdout captured.
    Fallback,
}

/// Commands that need the agent to act between prompts, which ACP v1 has no
/// way to express (an agent only speaks during a client-initiated turn).
const TERMINAL_ONLY: &[(&str, &str)] = &[
    ("/schedule", "scheduled goals fire on a timer, between prompts"),
    ("/unschedule", "scheduled goals fire on a timer, between prompts"),
    ("/bg", "background agents report back between prompts"),
    ("/agents", "background agents report back between prompts"),
    ("/remote", "the remote-control server feeds the terminal UI's input loop"),
];

/// Commands that only make sense for a terminal window.
const EDITOR_OWNED: &[(&str, &str)] = &[
    ("/new", "Start a new thread in your editor — each thread is a fresh zap session."),
    ("/exit", "Close the thread in your editor to end this session."),
    ("/quit", "Close the thread in your editor to end this session."),
];

fn split(input: &str) -> (&str, &str) {
    let input = input.trim();
    match input.split_once(char::is_whitespace) {
        Some((cmd, arg)) => (cmd, arg.trim()),
        None => (input, ""),
    }
}

/// The `available_commands_update` payload: built-in commands that work in an
/// editor, plus every skill as a direct `/<skill>` command.
pub fn available(session: &Session) -> Value {
    let mut seen = std::collections::HashSet::new();
    let mut commands: Vec<Value> = Vec::new();
    for (cmd, desc) in SLASH_COMMANDS {
        if desc.starts_with(SECTION_PREFIX) { continue; }
        let name = cmd.split_whitespace().next().unwrap_or(cmd);
        if TERMINAL_ONLY.iter().chain(EDITOR_OWNED).any(|(c, _)| *c == name) { continue; }
        if !seen.insert(name.to_string()) { continue; }
        commands.push(json!({
            "name": name.trim_start_matches('/'),
            "description": desc,
            "input": { "hint": "arguments (optional)" },
        }));
    }
    for skill in &session.skills {
        if seen.insert(format!("/{}", skill.name)) {
            commands.push(json!({
                "name": skill.name,
                "description": format!("run skill: {}", skill.name),
                "input": { "hint": "what to do with this skill" },
            }));
        }
    }
    json!({ "sessionUpdate": "available_commands_update", "availableCommands": commands })
}

/// Handle `input` (which starts with `/`). `out` emits markdown to the client.
pub async fn run(session: &mut Session, cwd: &mut PathBuf, input: &str, out: &mut dyn FnMut(&str)) -> Next {
    let (cmd, arg) = split(input);

    if let Some((_, why)) = TERMINAL_ONLY.iter().find(|(c, _)| *c == cmd) {
        out(&format!(
            "`{cmd}` isn't available inside an editor: {why}, and the editor protocol only lets an agent act in reply to a prompt. Run `zap` in a terminal to use it."
        ));
        return Next::Done;
    }
    if let Some((_, msg)) = EDITOR_OWNED.iter().find(|(c, _)| *c == cmd) {
        out(msg);
        return Next::Done;
    }

    match (cmd, arg) {
        ("/index", "") => {
            out("Indexing the codebase…\n\n");
            out(&index(session, cwd).await);
            Next::Done
        }
        ("/init", _) => init(session, cwd, arg, out).await,
        ("/provider", _) => {
            out(&provider(session, arg));
            Next::Done
        }
        ("/model", "") => {
            out(&models(session));
            Next::Done
        }
        ("/sessions", _) => {
            out(&sessions(session, cwd));
            Next::Done
        }
        ("/diff", _) => {
            out(&diff(cwd));
            Next::Done
        }
        ("/context", _) => {
            out(&context(session));
            Next::Done
        }
        ("/tasks", _) => tasks(arg, out),
        ("/goal", _) => goal(arg, out),
        _ => {
            let config = session.config.clone();
            match crate::tui::commands::handle_inline(session, input.trim(), &config) {
                Some(text) => {
                    if cmd == "/cd" {
                        if let Ok(dir) = std::env::current_dir() { *cwd = dir; }
                    }
                    if !text.is_empty() { out(&fenced(&text)); }
                    Next::Done
                }
                None => Next::Fallback,
            }
        }
    }
}

/// Terminal-formatted text (aligned columns, glyph bullets) as a code block,
/// so markdown rendering doesn't reflow it.
pub fn fenced(text: &str) -> String {
    format!("```text\n{}\n```\n", text.trim_end())
}

async fn index(session: &Session, cwd: &Path) -> String {
    let code_index = session.code_index.clone();
    let cwd = cwd.to_path_buf();
    tokio::task::spawn_blocking(move || crate::session::commands::run_init_indexing(&code_index, &cwd))
        .await
        .unwrap_or_else(|e| format!("Index error: {e}"))
}

/// `/init [languages]` — the TUI wizard's questions become an optional
/// argument (default: auto-detected language) and "yes, index now".
async fn init(session: &mut Session, cwd: &Path, arg: &str, out: &mut dyn FnMut(&str)) -> Next {
    let language = if arg.is_empty() {
        crate::session::commands::detect_project_type().to_string()
    } else {
        arg.to_string()
    };
    let languages: Vec<String> = language
        .split([',', ' '])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)
        .collect();
    out(&format!("Setting up this project for zap ({})… indexing the codebase.\n\n", languages.join(", ")));
    let index_section = index(session, cwd).await;
    let (output, llm_prompt) = session.cmd_init_direct(languages, Some(index_section), true);
    out(&fenced(&output));
    match llm_prompt {
        Some(prompt) => {
            out("\nAnalysing the codebase to fill `ZAP.md` — this may take a minute…\n\n");
            Next::Turn(prompt)
        }
        None => Next::Done,
    }
}

/// `/provider` lists configured providers; `/provider <slug> [model]` switches.
fn provider(session: &mut Session, arg: &str) -> String {
    let (slug, model) = split(arg);
    let cfg = &session.config;
    if slug.is_empty() {
        let mut slugs: Vec<&String> = cfg.all_providers.keys().collect();
        slugs.sort();
        let mut s = format!("**Current:** `{}` · `{}`\n\n**Configured providers:**\n", cfg.provider_slug, session.model);
        for name in slugs {
            let model = cfg.all_providers[name].model.as_deref().unwrap_or("(default model)");
            s.push_str(&format!("- `{name}` · {model}\n"));
        }
        s.push_str("\nSwitch with `/provider <name> [model]`. To add a provider or API key, run `zap` in a terminal and use `/provider` there — keys shouldn't be typed into a chat.");
        return s;
    }
    if !cfg.all_providers.contains_key(slug) {
        return format!(
            "`{slug}` isn't configured yet. Run `zap` in a terminal and add it with `/provider`, then switch here with `/provider {slug}`."
        );
    }
    let mut new_cfg = match Config::load_with_provider(Some(slug)) {
        Ok(c) => c,
        Err(e) => return format!("Could not load provider `{slug}`: {e}"),
    };
    new_cfg.tui_mode = true;
    if !model.is_empty() {
        new_cfg.model = model.to_string();
        if let Some(entry) = new_cfg.all_providers.get_mut(slug) {
            entry.model = Some(model.to_string());
        }
    }
    session.client = crate::llm_client::create_client(&new_cfg);
    session.model = new_cfg.model.clone();
    session.base_url = new_cfg.base_url.clone();
    session.config = new_cfg.clone();
    let saved = match new_cfg.save() {
        Ok(()) => String::new(),
        Err(e) => format!(" (not saved as default: {e})"),
    };
    format!("✓ Switched to `{slug}` · `{}`{saved}", session.model)
}

fn models(session: &Session) -> String {
    // Fetches the provider's live list with a blocking HTTP client, which
    // tokio only tolerates inside `block_in_place`.
    let models = tokio::task::block_in_place(|| {
        crate::tui::provider_picker::models_for_current_provider(&session.config)
    });
    let mut s = format!("**Current model:** `{}` ({})\n\n", session.model, session.config.provider_slug);
    let known: Vec<&String> = models.iter().filter(|m| m.as_str() != "Other…").collect();
    if !known.is_empty() {
        s.push_str("**Known models for this provider:**\n");
        for m in known { s.push_str(&format!("- `{m}`\n")); }
        s.push('\n');
    }
    s.push_str("Switch with `/model <name>` (this thread only). `/models` asks the server for its live list.");
    s
}

fn sessions(session: &Session, cwd: &Path) -> String {
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let rows = match session.store.recent_sessions_for_cwd(&cwd.display().to_string(), 20) {
        Ok(r) => r,
        Err(e) => return format!("sessions: {e}"),
    };
    if rows.is_empty() {
        return "No saved sessions for this project yet.".to_string();
    }
    let mut s = String::from("**Recent sessions in this project**\n\n");
    for (id, goal, model, ts) in rows {
        let goal: String = goal.chars().take(80).collect();
        s.push_str(&format!("- `#{id}` · {} · {model} — {goal}\n", ts.get(..16).unwrap_or(&ts)));
    }
    s.push_str("\nResume one from your editor's thread history.");
    s
}

fn git(cwd: &Path, args: &[&str]) -> String {
    std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn diff(cwd: &Path) -> String {
    const MAX: usize = 40_000;
    let unstaged = git(cwd, &["diff", "--no-color"]);
    let staged = git(cwd, &["diff", "--staged", "--no-color"]);
    if unstaged.trim().is_empty() && staged.trim().is_empty() {
        return "No diff available or not in a git repository.".to_string();
    }
    let mut s = String::new();
    for (title, body) in [("Staged", staged), ("Unstaged", unstaged)] {
        if body.trim().is_empty() { continue; }
        let mut cut = body.len().min(MAX);
        while !body.is_char_boundary(cut) { cut -= 1; }
        s.push_str(&format!("**{title}**\n```diff\n{}\n```\n", body[..cut].trim_end()));
        if cut < body.len() {
            s.push_str(&format!("_… truncated, {} more bytes_\n", body.len() - cut));
        }
    }
    s
}

/// `/context` — the TUI's interactive viewer, as a per-turn summary.
fn context(session: &Session) -> String {
    let msgs = &session.messages;
    let chars = |b: &ContentBlock| match b {
        ContentBlock::Text { text } => text.len(),
        ContentBlock::ToolUse { input, .. } => input.to_string().len(),
        ContentBlock::ToolResult { content, .. } => content.len(),
        ContentBlock::Thinking { thinking, .. } => thinking.len(),
        ContentBlock::Reasoning { content } => content.len(),
        ContentBlock::Image { .. } => 0,
    };
    let starts: Vec<usize> = msgs.iter().enumerate()
        .filter(|(_, m)| m.role == "user" && matches!(m.content.first(), Some(ContentBlock::Text { .. })))
        .map(|(i, _)| i)
        .collect();
    let mut s = format!(
        "**Context:** {}% full · {} messages · {} turns\n\n",
        session.context_fill_pct(), msgs.len(), starts.len()
    );
    for (n, &start) in starts.iter().enumerate() {
        let end = starts.get(n + 1).copied().unwrap_or(msgs.len());
        let tokens: usize = msgs[start..end].iter().flat_map(|m| &m.content).map(chars).sum::<usize>() / 4;
        let first = match msgs[start].content.first() {
            Some(ContentBlock::Text { text }) => text.lines().next().unwrap_or("").chars().take(70).collect::<String>(),
            _ => String::new(),
        };
        s.push_str(&format!("{}. ~{} tok — {}\n", n + 1, tokens, first));
    }
    s.push_str("\n`/compact` summarises history in place; `/clear` drops it.");
    s
}

/// `/tasks` lists task sessions; `/tasks <session> <n>` runs task `n`.
fn tasks(arg: &str, out: &mut dyn FnMut(&str)) -> Next {
    let files = crate::task_planner::discover_task_files();
    if files.is_empty() {
        out("No task sessions found. Task files live in `.zap/tasks/<session>/tasks.md`.");
        return Next::Done;
    }
    let (folder, number) = split(arg);
    if !folder.is_empty() {
        let Some(tf) = files.iter().find(|tf| tf.folder == folder) else {
            out(&format!("No task session named `{folder}`. Run `/tasks` to list them."));
            return Next::Done;
        };
        let Some(task) = number.parse::<usize>().ok()
            .and_then(|n| tf.tasks.iter().find(|t| t.number == n))
        else {
            out(&format!("Usage: `/tasks {folder} <task number>`"));
            return Next::Done;
        };
        out(&format!("▶ Executing task {} — {}\n\n", task.number, task.title));
        return Next::Turn(task.execution_prompt());
    }
    let mut s = String::new();
    for tf in &files {
        s.push_str(&format!("**{}** — {} ({}/{} done)\n", tf.folder, tf.goal, tf.done_count(), tf.tasks.len()));
        for t in &tf.tasks {
            s.push_str(&format!("- {} {}. {}\n", if t.is_done() { "✓" } else { "○" }, t.number, t.title));
        }
        s.push('\n');
    }
    s.push_str("Run one with `/tasks <session> <task number>`.");
    out(&s);
    Next::Done
}

/// `/goal <condition> [--max N]` — same parsing as the TUI.
fn goal(arg: &str, out: &mut dyn FnMut(&str)) -> Next {
    if arg.is_empty() || arg == "status" || arg == "stop" || arg == "cancel" {
        out("Usage: `/goal <condition> [--max N]` — zap keeps working turn by turn until the goal is met or N turns (default 20) are used. Stop it with your editor's stop button.\n\nExample: `/goal add unit tests for the auth module`");
        return Next::Done;
    }
    let (condition, max_turns) = match arg.find("--max") {
        Some(idx) => {
            let n = arg[idx + 5..].split_whitespace().next().and_then(|s| s.parse().ok()).unwrap_or(20);
            (arg[..idx].trim().to_string(), n)
        }
        None => (arg.to_string(), 20),
    };
    Next::Goal { condition, max_turns: max_turns.max(1) }
}

/// Prompt for goal turn `n` (1-based) — the TUI's wording.
pub fn goal_prompt(n: usize, max: usize, condition: &str) -> String {
    if n == 1 {
        format!("[Goal 1/{max}] {condition}\n\nWhen the goal is fully complete, end your response with exactly: ✓ DONE")
    } else {
        format!("[Goal {n}/{max}] Continue toward: {condition}. When fully done, end your response with: ✓ DONE")
    }
}

/// True when the last assistant message carries the goal-completion marker.
pub fn goal_done(session: &Session) -> bool {
    session.messages.iter().rev()
        .find(|m| m.role == "assistant")
        .is_some_and(|m| m.content.iter().any(|b| match b {
            ContentBlock::Text { text } => {
                text.contains("✓ DONE") || text.contains("✓DONE") || text.to_lowercase().contains("✓ done")
            }
            _ => false,
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_separates_command_and_argument() {
        assert_eq!(split("/model  gpt-x "), ("/model", "gpt-x"));
        assert_eq!(split("/cost"), ("/cost", ""));
    }

    #[test]
    fn goal_parses_condition_and_max() {
        let mut sink = |_: &str| {};
        match goal("add tests --max 3", &mut sink) {
            Next::Goal { condition, max_turns } => {
                assert_eq!(condition, "add tests");
                assert_eq!(max_turns, 3);
            }
            _ => panic!("expected Goal"),
        }
        assert!(matches!(goal("", &mut sink), Next::Done));
        assert!(matches!(goal("ship it", &mut sink), Next::Goal { max_turns: 20, .. }));
    }

    #[test]
    fn goal_prompts_match_tui_wording() {
        assert!(goal_prompt(1, 5, "x").starts_with("[Goal 1/5] x"));
        assert!(goal_prompt(2, 5, "x").starts_with("[Goal 2/5] Continue toward: x"));
    }

    #[test]
    fn fenced_wraps_terminal_text() {
        assert_eq!(fenced("a\n  b\n"), "```text\na\n  b\n```\n");
    }
}
