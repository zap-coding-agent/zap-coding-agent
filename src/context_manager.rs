use anyhow::Result;
use crate::config::Config;

pub fn build_system_prompt(config: &Config) -> Result<String> {
    build_system_prompt_with_skills(config, "")
}

/// Build the system prompt, optionally injecting pre-matched skill content.
pub fn build_system_prompt_with_skills(config: &Config, skill_block: &str) -> Result<String> {

    let mut sections: Vec<String> = Vec::new();

    // ── Identity ──────────────────────────────────────────────────────────────
    sections.push(format!(
        "You are a secure Rust AI coding agent (model: {}).\n\
         You help users accomplish coding and engineering tasks using tools.\n\
         You are precise, concise, and security-conscious.",
        config.model
    ));

    // ── Environment ───────────────────────────────────────────────────────────
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());
    let platform = std::env::consts::OS;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());

    sections.push(format!(
        "## Environment\n\
         - Platform : {}\n\
         - Shell    : {}\n\
         - CWD      : {}",
        platform, shell, cwd
    ));

    // ── Code navigation strategy ──────────────────────────────────────────────
    sections.push(
        "## Code Navigation Strategy (use in this order)\n\
         \n\
         The agent has a persistent AST-based code index (tree-sitter + SQLite) that \
         is much faster and more accurate than grep. Always prefer it:\n\
         \n\
         1. **`code_map`** — get the structural outline of a file or directory \
            (functions, structs, classes, line numbers). Use this first to orient yourself.\n\
         2. **`find_definition`** — jump directly to where a symbol is defined. \
            Backed by the AST index; returns exact file + line number.\n\
         3. **`search_code`** — pattern search across the codebase (ripgrep). \
            Use when the symbol name is unknown or for non-definition searches.\n\
         4. **`read_file` with `offset`/`limit`** — read only the lines you need \
            after you know the file and line number from the above tools.\n\
         \n\
         Never read an entire large file when `code_map` or `find_definition` \
         can give you the location first."
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
         **Shell commands:**\n\
         - Prefer targeted tools (`git_status`, `search_code`) over raw shell \
           commands when they cover the use case.\n\
         - Always provide a `description` when calling `shell` so the user \
           understands what the command will do before approving it.\n\
         - Never run commands that modify the system outside the working directory \
           without explicit user instruction.\n\
         - **Background processes:** When starting a long-running server, watcher, \
           or any process that doesn't exit on its own, ALWAYS use:\n\
           `nohup <cmd> > /tmp/<name>.log 2>&1 &`\n\
           Never use `cmd 2>&1 &` — that keeps the stdout pipe open and blocks zap \
           until the 60s timeout. Use nohup + redirect to a log file instead.\n\
         \n\
         **Search:**\n\
         - Use `find_definition` or `code_map` (AST index) before `search_code`.\n\
         - Use `list_directory` to understand project layout before diving into files."
            .to_string(),
    );

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
         - Do not repeat what you are about to do in text before doing it — \
           just do it using tools.\n\
         - After completing a task, give a short summary of what changed.\n\
         - Do not add unnecessary filler phrases like 'Certainly!' or \
           'Great question!'.\n\
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

    // ── Project context (CLAUDE.md) ───────────────────────────────────────────
    if let Some(claude_md) = load_claude_md() {
        sections.push(format!("## Project Context\n{}", claude_md));
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

/// Walk from cwd up to $HOME, loading CLAUDE.md at each level.
/// Also loads ~/.claude/CLAUDE.md as a global config layer.
/// Sections are ordered most-general → most-specific so that project-level
/// instructions appear last and carry more weight with the model.
fn load_claude_md() -> Option<String> {
    let home = std::env::var("HOME").ok().map(std::path::PathBuf::from);
    let cwd  = std::env::current_dir().ok()?;

    // Collect ancestor directories from cwd up to (and including) $HOME.
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    let mut cur = cwd.as_path();
    loop {
        dirs.push(cur.to_path_buf());
        // Stop at $HOME so we don't walk the entire filesystem.
        if home.as_deref() == Some(cur) {
            break;
        }
        match cur.parent() {
            Some(p) => cur = p,
            None    => break,
        }
    }
    // Reverse: most-general (home/ancestor) first, most-specific (cwd) last.
    dirs.reverse();

    let mut sections: Vec<String> = Vec::new();

    // Global layer: ~/.claude/CLAUDE.md
    if let Some(ref h) = home {
        let global = h.join(".claude").join("CLAUDE.md");
        if let Ok(contents) = std::fs::read_to_string(&global) {
            if !contents.trim().is_empty() {
                sections.push(format!("### {} (global)\n{}", global.display(), contents.trim()));
            }
        }
    }

    // Per-directory layers.
    for dir in &dirs {
        for name in &["CLAUDE.md", ".claude/CLAUDE.md"] {
            let path = dir.join(name);
            if let Ok(contents) = std::fs::read_to_string(&path) {
                if !contents.trim().is_empty() {
                    sections.push(format!("### {}\n{}", path.display(), contents.trim()));
                }
            }
        }
    }

    if sections.is_empty() { None } else { Some(sections.join("\n\n")) }
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
