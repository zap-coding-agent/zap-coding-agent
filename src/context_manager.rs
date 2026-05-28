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
         **Strict tool order — do not skip steps:**\n\
         \n\
         1. **`code_map`** — ALWAYS call this first on any file or directory before \
            reading it. Returns functions, structs, classes, and line numbers so you \
            know exactly which lines to read. Do NOT call `read_file` on a file you \
            have not yet `code_map`ped.\n\
         2. **`find_definition`** — when you know a symbol name, jump directly to its \
            definition. Saves a `search_code` + `read_file` round-trip.\n\
         3. **`search_code`** — pattern/regex search (ripgrep). Use only when the \
            symbol name is unknown or you need non-definition matches.\n\
         4. **`read_file` with `offset`/`limit`** — last resort, targeted. After \
            `code_map` tells you the line range, read only those lines.\n\
         \n\
         **`list_directory` — severely restricted:**\n\
         - Call it AT MOST ONCE per turn, only on the project root `'.'`\n\
         - It is non-recursive (one level only) — do NOT chain calls across subdirectories\n\
         - Do NOT use it to enumerate files; use `code_map` on directories instead\n\
         - Only use it when the project has no index and `code_map` returns nothing\n\
         \n\
         **The index is your primary tool.** `code_map` + `find_definition` cover \
         90% of navigation tasks. Reaching for `read_file` or `search_code` first \
         wastes tool calls and context — always check the index first.\n\
         \n\
         **If `code_map` or `find_definition` return 0 results:**\n\
         - The project may not be indexed — do NOT conclude it is empty.\n\
         - Fall back to ONE `list_directory '.'` call only, then `search_code` → `read_file`.\n\
         - Tell the user they can run `/index` to enable fast symbol lookup.\n\
         \n\
         **Never explore these directories** — dependencies/build output only: \
         `node_modules`, `target`, `dist`, `build`, `bin`, `obj`, \
         `out`, `.git`, `__pycache__`, `.venv`, `venv`, `coverage`, `.next`."
            .to_string(),
    );

    // ── Semantic search strategy ──────────────────────────────────────────────
    sections.push(
        "## Semantic Search Strategy\n\
         \n\
         When the user asks about a **concept** rather than an exact symbol name \
         (e.g. \"authentication\", \"caching\", \"SSO\", \"error handling\"), the code \
         may use completely different names (e.g. `sso_handler`, `iam_login`, `jwt_validate`).\n\
         \n\
         **Search in this order — index first, grep as last resort:**\n\
         1. `find_definition` for the 2–3 most likely candidate names (index hit = exact, done)\n\
         2. If those miss, `code_map` on the most relevant directory — \
            scan the symbol list to spot the real name\n\
         3. Only if `code_map` still doesn't reveal it: `search_code` with a regex \
            alternation of all candidates: `(auth|sso|login|iam|oauth)`\n\
         \n\
         **Always end your answer with one line explaining how you found it:**\n\
         - Index hit  : \"Found via index: `SsoHandler` at `auth/sso.ts:42`\"\n\
         - Fallback   : \"Not in index — found via search: `sso_handler` at `auth/sso.ts:42`\"\n\
         - Not found  : \"Not found in index or search — this feature may not be implemented yet.\"\n\
         \n\
         This makes your reasoning transparent: the user sees exactly what was tried, \
         what matched, and how confident the result is."
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
         - **Never use `shell` for directory listing or file discovery.** \
           Use `list_directory`, `glob_read`, or `code_map` instead — they are \
           faster, safer, and cannot hang on symlink loops.\n\
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
         - Always try `find_definition` or `code_map` before `search_code` or `read_file`.\n\
         - If you know the symbol name: `find_definition` → done.\n\
         - If you know the file: `code_map` → read only the relevant lines.\n\
         - `search_code` is for unknown symbol names or cross-file pattern matching only."
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
         - Use plain text, not excessive markdown headers, in conversational replies.\n\
         - **When a skill workflow is triggered, do not ask clarifying questions \
           and do not stop to announce upcoming steps.** Use tools to gather \
           everything you need. Only pause for a genuinely destructive action or \
           when the skill explicitly requires user input."
            .to_string(),
    );

    // ── Task tracking ─────────────────────────────────────────────────────────
    sections.push(
        "## Task Tracking\n\
         \n\
         Use `todo_write` and `todo_read` to plan and drive multi-step work.\n\
         \n\
         **When a skill workflow is triggered or the user gives a multi-step task \
         (3+ steps), always start by writing the full plan as todos — then execute \
         every item in the same turn without stopping between steps.** The todo list \
         is your execution contract: write it once, work through it completely.\n\
         \n\
         - **Plan first, then run:** call `todo_read` to check for an existing list, \
           then `todo_write` with every step from the skill or task. Use \
           `find_definition` / `code_map` / `search_code` to answer structural \
           questions as you go — the code index means you never have to ask the \
           user what already exists.\n\
         - **Status discipline:** mark an item `in_progress` before starting it, \
           `done` immediately when finished. Only one item `in_progress` at a time.\n\
         - **Do not stop between items** to check in, announce upcoming steps, or \
           wait for confirmation. Work through the entire list and present the full \
           result at the end.\n\
         - **Replace the whole list:** each `todo_write` call is a full replacement — \
           include every item, not just the changed ones.\n\
         - **When not to use it:** single-step tasks, quick answers, read-only queries."
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
                    "## Agent Memory\n\
                     These facts were saved in previous sessions:\n{facts}\n\n\
                     You can proactively persist cross-project facts that are worth \
                     remembering using `/memory set <key> <value>`.",
                    facts = facts
                ));
            } else {
                sections.push(
                    "## Agent Memory\n\
                     No facts saved yet. Use `/memory set <key> <value>` to persist \
                     cross-project facts (e.g. preferred patterns, team conventions, \
                     API endpoints) that should be available in future sessions."
                        .to_string(),
                );
            }
        }
    }

    // ── Project context (ZAP.md + .zap/understanding.md) ─────────────────────
    if let Some(zap_md) = load_zap_md(&config.context_paths) {
        sections.push(format!("## Project Context\n{}", zap_md));
    }
    // understanding.md — always inlined (capped at 4 kchars ≈ 1k tokens).
    // Used as technical reference when writing code, reviewing architecture,
    // or navigating the codebase — not as a script to read out to the user.
    let understanding = crate::project::load_understanding(4000);
    let has_real_analysis = understanding.as_deref().map(|u| {
        !u.contains("Run `/init`")
            && (u.contains("## Analysis")
                || u.contains("## Architecture")
                || u.contains("## Overview"))
    }).unwrap_or(false);

    if has_real_analysis {
        let note = "**Use this as technical background when writing code or navigating \
            the codebase. Do NOT recite it verbatim for user-facing questions — for general \
            queries (\"what is this?\", \"summarize\", \"overview\") answer in plain, \
            end-user-friendly language: what the product does and who it's for.**";
        sections.push(format!("## Project Reference\n{}\n{}", understanding.unwrap(), note));
    } else {
        // No /init analysis yet — give the LLM an active self-orientation routine
        // so it can produce high-quality answers through its own exploration.
        let stats_note = understanding
            .map(|u| format!("\n{}", u))
            .unwrap_or_default();
        sections.push(format!(
            "## Project Orientation{stats_note}\n\
             \n\
             No pre-computed analysis exists for this project yet. \
             **Before answering any question that requires project knowledge, \
             orient yourself in at most 4 tool calls:**\n\
             1. `code_map '.'` — one call gives the full project structure with symbols; \
                do NOT use `list_directory` to enumerate files\n\
             2. `read_file` on the manifest only (`Cargo.toml`, `package.json`, `go.mod`) \
                — for tech stack and build commands\n\
             3. `code_map` on 1-2 key source dirs if step 1 wasn't enough detail\n\
             4. `read_file` on the entry point (targeted lines only) if still unclear\n\
             \n\
             Answer from what you concretely discover — do not guess or fabricate \
             project details. Distinguish clearly between what you read from source \
             vs what you inferred from naming conventions.\n\
             \n\
             For general user queries (\"summarize\", \"what is this?\", \"overview\") \
             give a plain end-user-friendly description of what the product does and \
             who it's for — not a dump of internal implementation details."
        ));
    }
    // session_log.md — lazy hint (context.md is already injected above at startup)
    if std::path::Path::new(".zap/session_log.md").exists() {
        sections.push(
            "## Session History\n\
             `.zap/session_log.md` lists goals and files from past sessions. \
             Read it with `read_file` when the user asks about past work or recent changes."
                .to_string(),
        );
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
fn load_zap_md(context_paths: &[String]) -> Option<String> {
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

    // Extra context directories — opt-in via context_paths config.
    // Each configured path's .md files are loaded as always-on context,
    // sorted by filename. Frontmatter (--- ... ---) is stripped from the content.
    // Useful for: .kiro/steering, .claude/context, or any team-shared markdown docs.
    for raw_path in context_paths {
        let dir = expand_tilde(raw_path);
        if !dir.is_dir() { continue; }
        let mut file_sections: Vec<(String, String)> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") { continue; }
                let Ok(raw) = std::fs::read_to_string(&path) else { continue };
                let body = strip_frontmatter(&raw);
                if !body.trim().is_empty() {
                    let name = path.file_name().map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    file_sections.push((name, body.trim().to_string()));
                }
            }
        }
        if !file_sections.is_empty() {
            file_sections.sort_by(|a, b| a.0.cmp(&b.0));
            let joined = file_sections.iter()
                .map(|(name, body)| format!("#### {}\n{}", name, body))
                .collect::<Vec<_>>()
                .join("\n\n");
            sections.push(format!("### {} (context)\n{}", raw_path, joined));
        }
    }

    if sections.is_empty() { None } else { Some(sections.join("\n\n")) }
}

/// Strip YAML/TOML frontmatter (--- ... ---) from the top of a markdown file.
fn strip_frontmatter(raw: &str) -> &str {
    let s = raw.trim_start();
    if !s.starts_with("---") { return s; }
    let after = &s[3..];
    // Find the closing ---
    if let Some(pos) = after.find("\n---") {
        after[pos + 4..].trim_start_matches('\n')
    } else {
        s
    }
}

fn expand_tilde(p: &str) -> std::path::PathBuf {
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    std::path::PathBuf::from(p)
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
