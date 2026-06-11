/// Hook system: run user-defined shell scripts on lifecycle events.
///
/// Config files (merged, project wins over global):
///   ~/.zap/hooks.json   — global hooks
///   .zap/hooks.json     — project hooks
///
/// Hook entry:  { "matcher": "shell" | "*", "command": "path/to/script" }
/// Matcher is optional — omit or use "*" to match all tools.
///
/// PreToolUse exit codes:
///   0         — allow (stdout printed as info)
///   2         — block the tool call (stdout shown as warning)
///   other != 0 — allow but print a warning
///
/// Context is written to the hook's stdin as a single JSON object.
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::{Command, Stdio};

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HookEntry {
    /// Tool name to match, or "*" / absent = match everything.
    #[serde(default)]
    pub matcher: Option<String>,
    pub command: String,
    /// Optional human-readable description shown in /hooks list.
    #[serde(default)]
    pub description: Option<String>,
}

impl HookEntry {
    fn matches(&self, tool_name: &str) -> bool {
        match &self.matcher {
            None => true,
            Some(m) if m == "*" => true,
            Some(m) => m == tool_name,
        }
    }
}

/// What a PreToolUse hook decided.
pub enum HookDecision {
    Allow,
    Block(String),
}

/// All hooks loaded from config files.
#[derive(Debug, Default, Clone)]
pub struct HookRunner {
    pub pre_tool_use:       Vec<HookEntry>,
    pub post_tool_use:      Vec<HookEntry>,
    pub session_start:      Vec<HookEntry>,
    pub session_end:        Vec<HookEntry>,
    pub user_prompt_submit: Vec<HookEntry>,
}

// ── Loading ───────────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
struct HooksFile {
    #[serde(default)] pre_tool_use:       Vec<HookEntry>,
    #[serde(default)] post_tool_use:      Vec<HookEntry>,
    #[serde(default)] session_start:      Vec<HookEntry>,
    #[serde(default)] session_end:        Vec<HookEntry>,
    #[serde(default)] user_prompt_submit: Vec<HookEntry>,
}

fn load_file(path: &std::path::Path) -> HooksFile {
    let Ok(content) = std::fs::read_to_string(path) else { return HooksFile::default() };
    serde_json::from_str(&content).unwrap_or_else(|e| {
        crate::zap_warn!("hooks: failed to parse {}: {}", path.display(), e);
        HooksFile::default()
    })
}

impl HookRunner {
    /// Load and merge global (~/.zap/hooks.json) + project (.zap/hooks.json).
    /// Project hooks are appended last so they run after global ones.
    ///
    /// Global hooks are the user's own machine config and always load. Project
    /// hooks ship inside the repo and execute arbitrary shell commands, so they
    /// are only honored when the directory is trusted (see `crate::trust`).
    /// This prevents a cloned repo from running its `SessionStart` hook the
    /// instant you open it.
    pub fn load() -> Self {
        let global = dirs::home_dir()
            .map(|h| load_file(&h.join(".zap/hooks.json")))
            .unwrap_or_default();

        let project_path = std::path::Path::new(".zap/hooks.json");
        let project = if project_path.exists() && !crate::trust::project_trusted() {
            let skipped = load_file(project_path);
            if skipped.pre_tool_use.len()
                + skipped.post_tool_use.len()
                + skipped.session_start.len()
                + skipped.session_end.len()
                + skipped.user_prompt_submit.len()
                > 0
            {
                crate::zap_warn!(
                    "skipped project hooks in .zap/hooks.json (untrusted directory) — {}",
                    crate::trust::untrusted_hint()
                );
            }
            HooksFile::default()
        } else {
            load_file(project_path)
        };

        let merge = |mut a: Vec<HookEntry>, b: Vec<HookEntry>| { a.extend(b); a };

        Self {
            pre_tool_use:       merge(global.pre_tool_use,       project.pre_tool_use),
            post_tool_use:      merge(global.post_tool_use,      project.post_tool_use),
            session_start:      merge(global.session_start,      project.session_start),
            session_end:        merge(global.session_end,        project.session_end),
            user_prompt_submit: merge(global.user_prompt_submit, project.user_prompt_submit),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pre_tool_use.is_empty()
            && self.post_tool_use.is_empty()
            && self.session_start.is_empty()
            && self.session_end.is_empty()
            && self.user_prompt_submit.is_empty()
    }

    pub fn total(&self) -> usize {
        self.pre_tool_use.len()
            + self.post_tool_use.len()
            + self.session_start.len()
            + self.session_end.len()
            + self.user_prompt_submit.len()
    }

    // ── Event firers ─────────────────────────────────────────────────────────

    /// Fire SessionStart hooks.
    pub fn fire_session_start(&self) {
        let payload = serde_json::json!({ "event": "SessionStart" });
        for h in &self.session_start {
            run_hook(h, &payload, false);
        }
    }

    /// Fire SessionEnd hooks.
    pub fn fire_session_end(&self) {
        let payload = serde_json::json!({ "event": "SessionEnd" });
        for h in &self.session_end {
            run_hook(h, &payload, false);
        }
    }

    /// Fire UserPromptSubmit hooks.
    /// Returns Some(new_prompt) if any hook modified stdin → stdout (non-empty stdout).
    /// The *last* hook stdout wins if multiple hooks write output.
    pub fn fire_user_prompt_submit(&self, prompt: &str) -> Option<String> {
        let payload = serde_json::json!({ "event": "UserPromptSubmit", "prompt": prompt });
        let mut modified: Option<String> = None;
        for h in &self.user_prompt_submit {
            if let Some(out) = run_hook(h, &payload, false) {
                if !out.trim().is_empty() {
                    modified = Some(out.trim().to_string());
                }
            }
        }
        modified
    }

    /// Fire PreToolUse hooks for the given tool.
    /// Returns `Block(reason)` if any hook exits with code 2.
    pub fn fire_pre_tool_use(&self, tool_name: &str, tool_input: &serde_json::Value) -> HookDecision {
        let payload = serde_json::json!({
            "event": "PreToolUse",
            "tool_name": tool_name,
            "tool_input": tool_input,
        });
        for h in &self.pre_tool_use {
            if !h.matches(tool_name) { continue; }
            if let HookDecision::Block(reason) = run_pre_hook(h, &payload) {
                return HookDecision::Block(reason);
            }
        }
        HookDecision::Allow
    }

    /// Fire PostToolUse hooks for the given tool (informational — cannot block).
    pub fn fire_post_tool_use(&self, tool_name: &str, tool_input: &serde_json::Value, tool_output: &str) {
        let payload = serde_json::json!({
            "event": "PostToolUse",
            "tool_name": tool_name,
            "tool_input": tool_input,
            "tool_output": tool_output,
        });
        for h in &self.post_tool_use {
            if !h.matches(tool_name) { continue; }
            run_hook(h, &payload, false);
        }
    }
}

// ── Execution helpers ─────────────────────────────────────────────────────────

/// Build a platform-appropriate shell invocation for a hook command string.
/// Windows: powershell -NoProfile -NonInteractive -Command <cmd>
/// Unix:    sh -c <cmd>
fn hook_cmd(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("powershell");
        c.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    }
}

/// Run a hook, return stdout if successful. Logs warnings on non-zero exit.
fn run_hook(entry: &HookEntry, payload: &serde_json::Value, silent: bool) -> Option<String> {
    let stdin_bytes = serde_json::to_vec(payload).unwrap_or_default();

    let result = hook_cmd(&entry.command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let in_tui = crate::tui::channel::is_tui_mode();

    let mut child = match result {
        Ok(c) => c,
        Err(e) => {
            if !silent {
                let msg = format!("  ⚠ hook failed to start `{}`: {}", entry.command, e);
                if in_tui {
                    crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(format!("\n{msg}")));
                } else {
                    println!("{}", msg.yellow());
                }
            }
            return None;
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&stdin_bytes);
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            let msg = format!("  ⚠ hook error `{}`: {}", entry.command, e);
            if in_tui {
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(format!("\n{msg}")));
            } else {
                println!("{}", msg.yellow());
            }
            return None;
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() && !silent {
        let code = output.status.code().unwrap_or(-1);
        if code != 2 {
            let msg = format!("  ⚠ hook `{}` exited {}", entry.command, code);
            if in_tui {
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(format!("\n{msg}")));
            } else {
                println!("{}", msg.yellow());
            }
        }
        if !stderr.trim().is_empty() {
            let s = format!("  ┆ {}", stderr.trim());
            if in_tui {
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(format!("\n{s}")));
            } else {
                println!("{}", s.truecolor(180, 100, 80));
            }
        }
    } else if !stdout.trim().is_empty() && !silent {
        for line in stdout.trim().lines() {
            let s = format!("  ┆ {line}");
            if in_tui {
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(format!("\n{s}")));
            } else {
                println!("{}", s.truecolor(150, 200, 150));
            }
        }
    }

    Some(stdout)
}

/// Run a PreToolUse hook and return Block if exit code == 2.
fn run_pre_hook(entry: &HookEntry, payload: &serde_json::Value) -> HookDecision {
    let stdin_bytes = serde_json::to_vec(payload).unwrap_or_default();

    let result = hook_cmd(&entry.command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();

    let in_tui = crate::tui::channel::is_tui_mode();

    let mut child = match result {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("  ⚠ hook failed to start `{}`: {}", entry.command, e);
            if in_tui {
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(format!("\n{msg}")));
            } else {
                println!("{}", msg.yellow());
            }
            return HookDecision::Allow;
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&stdin_bytes);
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            let msg = format!("  ⚠ hook error `{}`: {}", entry.command, e);
            if in_tui {
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(format!("\n{msg}")));
            } else {
                println!("{}", msg.yellow());
            }
            return HookDecision::Allow;
        }
    };

    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if code == 2 {
        let reason = if !stdout.is_empty() { stdout } else { stderr };
        let reason = if reason.is_empty() { "blocked by hook".to_string() } else { reason };
        let msg = format!("  ⊘ hook blocked tool: {reason}");
        if in_tui {
            crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(format!("\n{msg}")));
        } else {
            println!("{}", msg.truecolor(220, 80, 80));
        }
        return HookDecision::Block(reason);
    }

    if !output.status.success() {
        let msg = format!("  ⚠ hook `{}` exited {} (tool still runs)", entry.command, code);
        if in_tui {
            crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(format!("\n{msg}")));
        } else {
            println!("{}", msg.yellow());
        }
    }
    if !stdout.is_empty() {
        for line in stdout.lines() {
            let s = format!("  ┆ {line}");
            if in_tui {
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(format!("\n{s}")));
            } else {
                println!("{}", s.truecolor(150, 200, 150));
            }
        }
    }

    HookDecision::Allow
}

// ── /hooks list helper ────────────────────────────────────────────────────────

pub fn print_hooks_list(runner: &HookRunner) {
    if runner.is_empty() {
        println!(
            "  No hooks configured. Create {} or {}",
            ".zap/hooks.json".cyan(),
            "~/.zap/hooks.json".cyan(),
        );
        println!();
        print_example();
        return;
    }

    let sections: &[(&str, &Vec<HookEntry>)] = &[
        ("SessionStart",      &runner.session_start),
        ("SessionEnd",        &runner.session_end),
        ("UserPromptSubmit",  &runner.user_prompt_submit),
        ("PreToolUse",        &runner.pre_tool_use),
        ("PostToolUse",       &runner.post_tool_use),
    ];

    for (event, hooks) in sections {
        if hooks.is_empty() { continue; }
        println!("  {}", event.truecolor(255, 210, 50).bold());
        for h in *hooks {
            let matcher = h.matcher.as_deref().unwrap_or("*");
            let desc = h.description.as_deref().unwrap_or("");
            println!(
                "    {} {}  {}",
                format!("[{}]", matcher).truecolor(130, 120, 155),
                h.command.cyan(),
                desc.truecolor(110, 110, 110),
            );
        }
    }
    println!();
}

fn print_example() {
    println!("  Example {}", ".zap/hooks.json".cyan());
    println!("  {}", r#"{
    "PreToolUse": [
      { "matcher": "shell", "command": "echo 'pre-shell hook'" }
    ],
    "PostToolUse": [
      { "matcher": "*", "command": "scripts/post-tool.sh" }
    ],
    "SessionStart": [
      { "command": "echo session started" }
    ],
    "UserPromptSubmit": [
      { "command": "cat" }
    ]
  }"#.truecolor(110, 110, 110));
}
