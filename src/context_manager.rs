use anyhow::Result;
use crate::config::Config;

pub fn build_system_prompt(config: &Config) -> Result<String> {
    build_system_prompt_with_skills(config, "")
}

/// Minimal system prompt for casual/greeting turns — omits code-nav, tool-policy,
/// sub-agent, security, git-status, and ZAP.md sections. Saves ~6-8k tokens.
pub fn build_casual_system_prompt(config: &Config) -> String {
    format!(
        "You are a helpful AI coding assistant (model: {}).\n\
         Be concise and conversational. Do not add filler phrases.",
        config.model
    )
}

/// Build the system prompt, optionally injecting pre-matched skill content.
pub fn build_system_prompt_with_skills(config: &Config, skill_block: &str) -> Result<String> {

    let mut sections: Vec<String> = Vec::new();

    // ── Identity ──────────────────────────────────────────────────────────────
    let lang_hint = crate::project::load_project_meta()
        .and_then(|m| if m.language.is_empty() { None } else { Some(m.language.join(", ")) })
        .map(|l| format!(" ({l})"))
        .unwrap_or_default();
    sections.push(format!(
        "You are a secure AI coding agent{lang_hint} (model: {}).\n\
         You help users accomplish coding and engineering tasks using tools.\n\
         You are precise, concise, and security-conscious.",
        config.model
    ));

    // ── Environment ───────────────────────────────────────────────────────────
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());
    let platform = std::env::consts::OS;
    let shell = if cfg!(windows) {
        "PowerShell".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())
    };

    sections.push(format!(
        "## Environment\n\
         - Platform : {}\n\
         - Shell    : {}\n\
         - CWD      : {}",
        platform, shell, cwd
    ));

    // ── Code navigation strategy ──────────────────────────────────────────────
    sections.push(
        "## Code Navigation Strategy\n\
         \n\
         Try tools in this order — each is faster than the next:\n\
         \n\
         1. **`code_map`** — structural outline of a file or directory \
            (functions, structs, classes, line numbers).\n\
         2. **`find_definition`** — jump directly to where a symbol is defined.\n\
         3. **`search_code`** — pattern search (ripgrep). Use when the symbol name \
            is unknown or for non-definition searches.\n\
         4. **`list_directory`** — understand project layout before diving into files.\n\
         5. **`read_file` with `offset`/`limit`** — read only the lines you need.\n\
         \n\
         **If `code_map` or `find_definition` return 0 results or an empty list:**\n\
         - The project may not be indexed yet — do NOT conclude the project is empty.\n\
         - Fall back immediately to `list_directory` to explore the structure, \
           then `search_code` or `read_file` to navigate.\n\
         - Tell the user they can run `/index` to enable fast symbol lookup.\n\
         \n\
         **Never explore these directories** — they contain dependencies/build output, \
         not source code: `node_modules`, `target`, `dist`, `build`, `bin`, `obj`, \
         `out`, `.git`, `__pycache__`, `.venv`, `venv`, `coverage`, `.next`."
            .to_string(),
    );

    // ── Tool usage policy ─────────────────────────────────────────────────────
    sections.push(
        "## Tool Usage Policy\n\
         \n\
         **Reading files:**\n\
         - Always read a file before editing it. Never assume its contents.\n\
         - Use `offset` + `limit` on `read_file` for large files. \
           The output includes line numbers; reference them in `edit_file` calls.\n\
         \n\
         **Editing files:**\n\
         - Prefer `edit_file` over `write_file` for any change to an existing file.\n\
         - `old_string` must match exactly — copy it from the `read_file` output, \
           including all whitespace and indentation.\n\
         - If `edit_file` fails because `old_string` is not unique, add more lines \
           of surrounding context to make it unambiguous.\n\
         - Only use `write_file` when creating a new file or intentionally \
           replacing 100% of an existing file's content.\n\
         \n\
         **Git commands — use `shell` directly:**\n\
         - `git status --short && git log --oneline -10` — working tree + recent history\n\
         - `git diff` / `git diff --cached` — unstaged / staged changes\n\
         - `git pull` / `git pull --rebase` — sync from remote\n\
         - `git add -p`, `git commit -m \"…\"`, `git push` — stage and commit\n\
         Always include a `description` in the shell call so the user knows what runs.\n\
         \n\
         **Shell commands:**\n\
         - Prefer targeted tools (`search_code`, `code_map`, `find_definition`) \
           over `shell` for code navigation.\n\
         - Always provide a `description` when calling `shell` so the user \
           understands what the command will do before approving it.\n\
         - Never run commands that modify the system outside the working directory \
           without explicit user instruction.\n\
         - **On Windows the shell is PowerShell.** Use PowerShell syntax: \
           `Get-ChildItem` (or `ls`), `Start-Sleep -Seconds N` (or `sleep N`), \
           `$env:VAR` for env vars, `cmd /C` only when explicitly needed for \
           cmd.exe-specific behaviour. Do NOT use bash syntax (`&&`, `||`, \
           `$(...)`, `nohup`) on Windows — use PowerShell equivalents.\n\
         - **Background processes:**\n\
           On Linux/macOS: `nohup <cmd> > /tmp/<name>.log 2>&1 &`\n\
           On Windows (PowerShell): `Start-Process powershell -ArgumentList \"-NoProfile -NonInteractive -Command <cmd>\" -RedirectStandardOutput C:\\tmp\\out.log -WindowStyle Hidden`\n\
           Never run a long-lived process in the foreground — it will time out.\n\
         \n\
         **Search:**\n\
         - Use `find_definition` or `code_map` (AST index) before `search_code`.\n\
         - Use `list_directory` to understand project layout before diving into files."
            .to_string(),
    );

    // ── Sub-agent orchestration ───────────────────────────────────────────────
    if config.agent_depth > 0 {
        sections.push(
            "## Sub-Agent Orchestration\n\
             \n\
             You can spawn parallel sub-agents with `spawn_agent`. Each sub-agent is a \
             full agent loop with its own message history and all tools. Multiple \
             `spawn_agent` calls in **one response** execute in parallel — the main \
             session only resumes after ALL complete.\n\
             \n\
             **Spawn when ALL hold:**\n\
             - The task has ≥2 truly independent sub-goals (no shared file writes)\n\
             - Each sub-goal is non-trivial (needs ≥1 tool call, not just a question)\n\
             - Results can be synthesised without one agent needing the other's intermediate output\n\
             \n\
             **Proactively propose spawning** before issuing the calls, e.g.:\n\
             > \"This has two independent parts — I'll run them in parallel:\\n\
             >   • Agent 1: analyse src/auth.rs for security issues\\n\
             >   • Agent 2: analyse src/api.rs for security issues\"\n\
             \n\
             **Trigger patterns** — consider spawning when the user asks to:\n\
             - \"analyse / review X, Y, Z\" where X, Y, Z are independent files or modules\n\
             - \"fix all issues in these files\" — one agent per file\n\
             - \"implement feature A and feature B\" (if truly independent)\n\
             - \"run tests AND update the docs\" — parallel work streams\n\
             - \"refactor module A, B, C\" — one agent per module\n\
             \n\
             **Anti-patterns (never spawn for these):**\n\
             - Sequential dependency: \"read file A, then edit B based on it\"\n\
             - Trivial tasks: reading one file, a single small edit, answering a question\n\
             - Overlapping writes: two agents editing the same file → race condition\n\
             \n\
             **After all sub-agents complete:** synthesise their findings into a coherent \
             reply. Do not dump each agent's output verbatim — summarise what changed and why.\n\
             \n\
             Use `files_in_scope` in each `spawn_agent` call so overlapping file access \
             is visible. Pass relevant parent findings through `context`."
                .to_string(),
        );
    }

    // ── Security rules ────────────────────────────────────────────────────────
    sections.push(
        "## Security Rules (non-negotiable)\n\
         \n\
         1. Never force-push to main or master (`git push --force origin main`).\n\
         2. Never skip pre-commit hooks (`--no-verify`).\n\
         3. Never delete files or directories without explicit user instruction.\n\
         4. Never write secrets, API keys, or passwords into files.\n\
         5. Never execute a command that could affect systems outside the \
            current repository without asking first.\n\
         6. When in doubt about a destructive action, stop and ask the user."
            .to_string(),
    );

    // ── Response style ────────────────────────────────────────────────────────
    sections.push(
        "## Response Style\n\
         \n\
         - Be concise. One to three sentences per update is enough.\n\
         - Do not narrate what you are about to do — just do it with tools.\n\
         - **Always produce a text response.** After every tool call (or set of \
           tool calls), write at least one sentence summarising what you found or \
           what changed. Never end a turn with only tool results and no text.\n\
         - If a tool returned an error or no output, say so explicitly.\n\
         - Do not add filler phrases like 'Certainly!' or 'Great question!'.\n\
         - Use plain text, not excessive markdown headers, in conversational replies."
            .to_string(),
    );

    // ── Agent memory (persistent key-value facts) ─────────────────────────────
    if let Ok(store) = crate::persistence::init() {
        if let Ok(entries) = store.all_memory() {
            if !entries.is_empty() {
                let facts = entries
                    .iter()
                    .map(|(k, v)| format!("- {}: {}", k, v))
                    .collect::<Vec<_>>()
                    .join("\n");
                sections.push(format!(
                    "## Agent Memory\nThese facts were saved in previous sessions:\n{}", facts
                ));
            }
        }
    }

    // ── Project context (ZAP.md + .zap/understanding.md) ─────────────────────
    if let Some(zap_md) = load_zap_md() {
        sections.push(format!("## Project Context\n{}", zap_md));
    }
    // understanding.md — always inlined (capped at 4 kchars ≈ 1k tokens).
    // The auto-stats block is always useful context; the Analysis section is
    // included when /init has run. Without inlining, the model answers summary
    // questions from guesswork instead of real project data.
    if let Some(understanding) = crate::project::load_understanding(4000) {
        let has_real_analysis = !understanding.contains("Run `/init`")
            && (understanding.contains("## Analysis")
                || understanding.contains("## Architecture")
                || understanding.contains("## Overview"));
        if has_real_analysis {
            sections.push(format!("## Project Overview\n{}", understanding));
        } else {
            // Stats only — no deep analysis yet. Still inline so the model
            // knows file/symbol counts, languages, and top-level modules.
            sections.push(format!(
                "## Project Overview\n{}\n\
                 *For a detailed technical analysis run `/init`.*",
                understanding
            ));
        }
    }
    // On-demand knowledge files — context.md and session_log.md are session-
    // specific and change every turn, so keep them as lazy hints.
    {
        let mut hints: Vec<&str> = Vec::new();
        if std::path::Path::new(".zap/context.md").exists() {
            hints.push("- `.zap/context.md` — last session: goal, files touched, what's next (read when asked to resume, \"where were we\", or what to work on next)");
        }
        if std::path::Path::new(".zap/session_log.md").exists() {
            hints.push("- `.zap/session_log.md` — history of past sessions (read when asked about past work or recent changes)");
        }
        if !hints.is_empty() {
            sections.push(format!(
                "## Session History Files\nRead these with `read_file` only when relevant:\n{}",
                hints.join("\n")
            ));
        }
    }

    // ── Git status ────────────────────────────────────────────────────────────
    if std::path::Path::new(".git").exists() {
        if let Some(status) = git_status_summary() {
            sections.push(format!("## Current Git Status\n```\n{}\n```", status));
        }
    }

    // ── Active skills (lazy-injected, only when triggered) ────────────────────
    if !skill_block.is_empty() {
        sections.push(skill_block.to_string());
    }

    Ok(sections.join("\n\n"))
}

/// Walk from cwd up to the git root (not beyond) loading ZAP.md at each level,
/// falling back to CLAUDE.md for projects that haven't migrated yet.
/// Also loads ~/.zap/ZAP.md as a global layer.
/// Sections ordered most-general → most-specific so project instructions take priority.
fn load_zap_md() -> Option<String> {
    let home = home_dir();
    let cwd  = std::env::current_dir().ok()?;

    // Find git root — stop the upward walk there so a parent repo's CLAUDE.md
    // cannot bleed into a child project (e.g. zap source bleeding into a test app).
    let git_root: Option<std::path::PathBuf> = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(&cwd)
        .output()
        .ok()
        .and_then(|o| if o.status.success() {
            String::from_utf8(o.stdout).ok()
                .map(|s| std::path::PathBuf::from(s.trim()))
        } else {
            None
        });

    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    let mut cur = cwd.as_path();
    loop {
        dirs.push(cur.to_path_buf());
        // Stop at the git root — never walk into an unrelated parent project.
        if git_root.as_deref() == Some(cur) { break; }
        if home.as_deref() == Some(cur) { break; }
        match cur.parent() {
            Some(p) => cur = p,
            None    => break,
        }
    }
    dirs.reverse();

    let mut sections: Vec<String> = Vec::new();

    // Global layer: ~/.zap/ZAP.md (also checks ~/.claude/CLAUDE.md for compat)
    if let Some(ref h) = home {
        for global_path in &[h.join(".zap").join("ZAP.md"), h.join(".claude").join("CLAUDE.md")] {
            if let Ok(contents) = std::fs::read_to_string(global_path) {
                if !contents.trim().is_empty() {
                    sections.push(format!("### {} (global)\n{}", global_path.display(), contents.trim()));
                    break; // use whichever exists first
                }
            }
        }
    }

    // Per-directory layers: ZAP.md preferred, CLAUDE.md as fallback.
    for dir in &dirs {
        for name in &["ZAP.md", "CLAUDE.md", ".zap/ZAP.md", ".claude/CLAUDE.md"] {
            let path = dir.join(name);
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if !contents.trim().is_empty() {
                    sections.push(format!("### {}\n{}", path.display(), contents.trim()));
                    break; // prefer ZAP.md over CLAUDE.md in same dir
                }
            }
        }
    }

    if sections.is_empty() { None } else { Some(sections.join("\n\n")) }
}

/// Return the user's home directory.
/// Checks $HOME first (Unix), then %USERPROFILE% (Windows), then %HOMEDRIVE%+%HOMEPATH%.
fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .or_else(|_| {
            let drive = std::env::var("HOMEDRIVE").unwrap_or_default();
            let path  = std::env::var("HOMEPATH").unwrap_or_default();
            if drive.is_empty() && path.is_empty() { Err(std::env::VarError::NotPresent) }
            else { Ok(format!("{}{}", drive, path)) }
        })
        .ok()
        .map(std::path::PathBuf::from)
}

fn git_status_summary() -> Option<String> {
    let mut child = std::process::Command::new("git")
        .args(["status", "--short"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;

    // Kill and skip if git takes more than 2 seconds (large repo).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                return None;
            }
            _ => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }

    let out = child.wait_with_output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}
