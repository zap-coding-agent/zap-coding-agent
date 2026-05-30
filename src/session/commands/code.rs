use colored::Colorize;
use super::super::Session;

impl Session {
    pub fn cmd_init(&mut self) -> Option<String> {
        let project_type = detect_project_type();
        let cfg = crate::ui::inquire_render_config();

        println!(
            "  {} Detected project type: {}",
            "◌".dimmed(),
            project_type.cyan(),
        );
        let language_input = inquire::Text::new("Language(s) for this project:")
            .with_initial_value(project_type)
            .with_render_config(cfg)
            .prompt()
            .unwrap_or_else(|_| project_type.to_string());
        let languages: Vec<String> = language_input
            .split([',', ' '])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect();

        println!(
            "  {} Indexing lets zap find symbols and definitions instantly without reading every file.",
            "◌".dimmed(),
        );
        let do_index = inquire::Confirm::new("Index this project now? (recommended, ~10s)")
            .with_default(true)
            .prompt()
            .unwrap_or(true);
        if do_index {
            self.cmd_index("");
            crate::project::mark_indexed();
        }

        let cwd_name = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "project".to_string());
        let meta = crate::project::ProjectMeta {
            name: cwd_name,
            language: languages,
            indexed: do_index,
            indexed_at: if do_index { Some(chrono::Utc::now().to_rfc3339()) } else { None },
            initialized_at: Some(chrono::Utc::now().to_rfc3339()),
        };
        if let Err(e) = crate::project::save_project_meta(&meta) {
            println!("  {} Could not write project.json: {}", "✗".red(), e);
        } else {
            println!("  {} .zap/project.json written.", "✓".green());
        }

        let zap_md = std::path::Path::new("ZAP.md");
        if zap_md.exists() {
            println!("  {} ZAP.md already exists — skipping template.", "◌".dimmed());
            println!(
                "  {} Project initialized. zap will remember this project.",
                "✓".green(),
            );
            return None;
        }
        let template = generate_zap_md_template(project_type);
        match std::fs::write("ZAP.md", &template) {
            Ok(_) => {
                println!("  {} Created ZAP.md for {} project.", "✓".green(), project_type.cyan());
                println!(
                    "  {} Project initialized. zap will remember this project.",
                    "✓".green(),
                );
                println!("  {} Asking the agent to analyse the repo and fill in ZAP.md…", "⚡".bright_yellow());
                Some(
                    "I just created ZAP.md with a template. Please read the project \
                     source files and fill in every section of ZAP.md accurately: \
                     Overview, Build & Test commands, Code Style conventions, Architecture, \
                     Important Files, and Do Not Touch sections. Use edit_file to update ZAP.md \
                     in place with real information from the repo. Also create \
                     .zap/understanding.md with a concise technical summary: main modules, \
                     key data flows, important patterns, and any non-obvious constraints."
                        .to_string(),
                )
            }
            Err(e) => { println!("  {} Could not write ZAP.md: {}", "✗".red(), e); None }
        }
    }

    /// TUI-native init: takes wizard choices, returns (output_text, optional_llm_prompt).
    /// No inquire prompts — all input was collected by the TUI wizard overlay.
    pub fn cmd_init_direct(
        &mut self,
        languages: Vec<String>,
        do_index: bool,
        do_understand: bool,
    ) -> (String, Option<String>) {
        let project_type = detect_project_type();
        let lang_label = if languages.is_empty() {
            project_type.to_string()
        } else {
            languages.join(", ")
        };
        let mut sections: Vec<String> = Vec::new();

        if do_index {
            let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let index_section = match self.code_index.lock() {
                Ok(mut guard) => match guard.index_dir(&cwd) {
                    Ok((new_files, new_syms)) => {
                        crate::project::mark_indexed();
                        let (total_files, total_syms) = guard.total_stats().unwrap_or((new_files, new_syms));
                        let lang_counts = guard.stats_by_language().unwrap_or_default();
                        let db_kb = cwd.join(".zap").join("code.db")
                            .metadata().map(|m| m.len() / 1024).unwrap_or(0);
                        let mut s = format!(
                            "Code index\n  {} files · {} symbols indexed",
                            total_files, total_syms
                        );
                        if !lang_counts.is_empty() {
                            let breakdown: Vec<String> = lang_counts.iter()
                                .map(|(l, n)| format!("{} ({})", l, n))
                                .collect();
                            s.push_str(&format!("\n  Languages: {}", breakdown.join(", ")));
                        }
                        if db_kb > 0 {
                            s.push_str(&format!("\n  DB: {} KB · .zap/code.db", db_kb));
                        }
                        s.push_str("\n  Stored in .zap/code.db (SQLite, local only — your code never leaves your machine)");
                        s.push_str("\n  Auto-updates: every 2 min while running · at session end");
                        s.push_str("\n  Run /index any time to refresh manually");
                        s
                    }
                    Err(e) => format!("Index error: {}", e),
                },
                Err(_) => "Index busy — run /index manually".to_string(),
            };
            sections.push(index_section);
        }

        let cwd_name = std::env::current_dir()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "project".to_string());
        let meta = crate::project::ProjectMeta {
            name:           cwd_name.clone(),
            language:       languages,
            indexed:        do_index,
            indexed_at:     if do_index { Some(chrono::Utc::now().to_rfc3339()) } else { None },
            initialized_at: Some(chrono::Utc::now().to_rfc3339()),
        };
        if let Err(e) = crate::project::save_project_meta(&meta) {
            sections.push(format!("✗ Could not write project.json: {}", e));
        }

        let zap_md = std::path::Path::new("ZAP.md");
        let created_zap_md = if !zap_md.exists() {
            let template = generate_zap_md_template(project_type);
            std::fs::write("ZAP.md", &template).is_ok()
        } else {
            false
        };

        let mut created = Vec::new();
        created.push("✓ .zap/project.json — language, index state, timestamps".to_string());
        if created_zap_md {
            created.push("✓ ZAP.md — project instructions loaded into every session".to_string());
        } else {
            created.push("· ZAP.md already exists".to_string());
        }
        if do_understand {
            created.push("✓ .zap/understanding.md — technical deep-dive (being written…)".to_string());
        }
        sections.push(format!("Files\n{}", created.join("\n")));

        if do_index {
            sections.push(
                "Analysis method\n\
                 Everything above is grounded in your actual source code:\n\
                 ◎ tree-sitter AST — symbols parsed directly from files, not inferred\n\
                 ◎ grep / search  — pattern matches against real file content\n\
                 ◎ file reads     — key files read in full for context\n\
                 The index is deterministic: same code → same results, every time.\n\
                 If you refactor or add files, /index refreshes it instantly."
                .to_string(),
            );
        }

        let output = format!(
            "Project '{}' initialized  ({})\n\n{}",
            cwd_name,
            lang_label,
            sections.join("\n\n")
        );

        let llm_prompt = if do_understand {
            Some(
                "I just ran /init on this project. Your job is to build a NAVIGATION MAP \
                 so future turns can go straight to the right file — zero guesswork.\n\
                 \n\
                 GOAL: produce a lookup table, not a code review. The question to answer \
                 is 'where do I go for X?' — not 'how does X work internally?'.\n\
                 \n\
                 TOOLS TO USE (in order):\n\
                 1. `list_directory '.'` — one call, see top-level layout\n\
                 2. `code_map` on each key source directory (src/, server/, lib/, app/, etc.) \
                    — this gives you all symbols with file paths and line numbers in one call. \
                    This IS the map. Use it instead of reading files.\n\
                 3. `read_file` on manifest only (Cargo.toml / package.json / go.mod) — \
                    1 call, for tech stack and build commands\n\
                 4. `read_file` on entry point only if code_map didn't make it clear — \
                    1 call max\n\
                 Then write both files. Do not read more files than this.\n\
                 \n\
                 DO NOT:\n\
                 - Read every file (that is code review, not navigation)\n\
                 - Use glob_read **/* or list_directory recursively\n\
                 - Enumerate files with sizes\n\
                 - Write narrative descriptions of how things work internally\n\
                 \n\
                 OUTPUT 1 — .zap/understanding.md, structured as:\n\
                 ## Entry Points\n\
                 (where does the app start, where do HTTP requests arrive, CLI main, etc.)\n\
                 ## Module Map\n\
                 (table: Module | Directory | Owns — one row per top-level source dir)\n\
                 ## Where To Find X\n\
                 (lookup: 'adding a route → X', 'DB schema → Y', 'auth logic → Z', etc.)\n\
                 ## Non-Obvious Constraints\n\
                 (things that would surprise a new dev: naming conventions, banned patterns, \
                 architectural rules not visible from file names)\n\
                 \n\
                 OUTPUT 2 — ZAP.md:\n\
                 Fill Overview, Build & Test (exact commands), Code Style, Architecture \
                 (reference the Module Map), Important Files (5-8 max with one-line reason \
                 each), Do Not Touch. Facts only.\n\
                 \n\
                 Start reply: 'Analysed via: [tools used]'."
                .to_string(),
            )
        } else {
            None
        };

        (output, llm_prompt)
    }

    /// Write `.zap/context.md` and append to `.zap/session_log.md` at session end (sync, no LLM).
    pub fn save_context(&self) {
        self.save_context_inner(None);
    }

    /// Ask the LLM for a brief "What's next" summary, then save context with it.
    pub async fn save_context_with_summary(&self) {
        if self.turn_count == 0 { return; }
        println!("  {} Saving session… (5s)", "◎".truecolor(180, 175, 210));
        let whats_next = self.summarize_whats_next().await;
        self.save_context_inner(whats_next.as_deref());
    }

    /// Build a short "What's next" summary from recent conversation history via LLM.
    async fn summarize_whats_next(&self) -> Option<String> {
        use crate::llm_client::{Message, ContentBlock};
        use std::time::Duration;

        if self.turn_count == 0 { return None; }

        // Collect last 10 text messages (user + assistant, no tool results).
        let recent: Vec<String> = self.messages.iter().rev()
            .filter_map(|m| {
                let texts: Vec<&str> = m.content.iter().filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                }).collect();
                if texts.is_empty() { None } else { Some(format!("[{}] {}", m.role, texts.join(" "))) }
            })
            .take(10)
            .collect::<Vec<_>>()
            .into_iter().rev().collect();

        if recent.is_empty() { return None; }

        let transcript = recent.join("\n\n");
        let prompt = format!(
            "Based on this conversation, write 1-3 concise bullet points describing \
             what should be worked on next in this project. Be specific: mention file names, \
             function names, or features. No preamble.\n\n{transcript}"
        );

        let result = tokio::time::timeout(
            Duration::from_secs(5),
            self.client.send(
                "You summarize software development sessions into actionable next-step notes.",
                &[Message::user_text(prompt)],
                &[],
                None,
                0,
            ),
        ).await;

        match result {
            Ok(Ok(resp)) => {
                let text: String = resp.content.iter().filter_map(|b| match b {
                    ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                }).collect::<Vec<_>>().join("\n");
                let trimmed = text.trim().to_string();
                if trimmed.is_empty() { None } else { Some(trimmed) }
            }
            _ => None,
        }
    }

    fn save_context_inner(&self, whats_next: Option<&str>) {
        if self.turn_count == 0 { return; }
        let goal = self.store
            .get_session_goal(self.session_id)
            .unwrap_or_else(|| "(untitled session)".to_string());
        if let Err(e) = crate::project::save_session_context(
            self.session_id,
            &goal,
            &self.files_changed,
            whats_next,
        ) {
            crate::log::write("WARN ", &format!("could not write context.md: {}", e));
        }
        if let Err(e) = crate::project::append_session_log(
            self.session_id,
            &goal,
            &self.files_changed,
        ) {
            crate::log::write("WARN ", &format!("could not update session_log.md: {}", e));
        }
        if let Ok(json) = serde_json::to_string(&self.messages) {
            if let Err(e) = self.store.save_messages(self.session_id, &json) {
                crate::log::write("WARN ", &format!("could not save messages: {}", e));
            }
        }
        let (files, symbols, langs): (usize, usize, Vec<(String, usize)>) =
            self.code_index.lock().ok().and_then(|mut guard| {
                let cwd = std::env::current_dir().ok()?;
                let _ = guard.index_dir(&cwd);
                let (f, s) = guard.total_stats().ok()?;
                let l = guard.stats_by_language().ok().unwrap_or_default();
                Some((f, s, l))
            }).unwrap_or_default();
        if let Err(e) = crate::project::ensure_understanding_md(
            std::env::current_dir().ok().and_then(|d| d.file_name().map(|n| n.to_string_lossy().to_string())),
            files, symbols, &langs,
        ) {
            crate::log::write("WARN ", &format!("could not ensure understanding.md: {}", e));
        }
    }
}

// ── /init helpers (no Session dependency) ────────────────────────────────────

pub fn detect_project_type() -> &'static str {
    if std::path::Path::new("Cargo.toml").exists()            { return "rust"; }
    if std::path::Path::new("go.mod").exists()                { return "go"; }
    if std::path::Path::new("package.json").exists() {
        if std::path::Path::new("tsconfig.json").exists()     { return "typescript"; }
        return "javascript";
    }
    if std::path::Path::new("pyproject.toml").exists()
        || std::path::Path::new("setup.py").exists()
        || std::path::Path::new("requirements.txt").exists()  { return "python"; }
    if std::path::Path::new("pom.xml").exists()               { return "java"; }
    if std::path::Path::new("build.gradle").exists()
        || std::path::Path::new("build.gradle.kts").exists()  { return "kotlin"; }
    if std::path::Path::new("*.swift").exists()
        || std::path::Path::new("Package.swift").exists()     { return "swift"; }
    if std::path::Path::new("CMakeLists.txt").exists()
        || std::path::Path::new("Makefile").exists() {
        if std::fs::read_dir(".")
            .ok()
            .map(|d| d.filter_map(|e| e.ok())
                .any(|e| e.path().extension()
                    .map(|x| x == "cpp" || x == "cc" || x == "cxx")
                    .unwrap_or(false)))
            .unwrap_or(false)
        {
            return "c++";
        }
        return "c";
    }
    let ext_counts = {
        let mut m: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        if let Ok(entries) = std::fs::read_dir(".") {
            for e in entries.flatten() {
                if let Some(ext) = e.path().extension().and_then(|x| x.to_str()) {
                    match ext {
                        "rs"  => *m.entry("rust").or_default()       += 1,
                        "go"  => *m.entry("go").or_default()          += 1,
                        "py"  => *m.entry("python").or_default()      += 1,
                        "ts"  => *m.entry("typescript").or_default()  += 1,
                        "js"  => *m.entry("javascript").or_default()  += 1,
                        "rb"  => *m.entry("ruby").or_default()        += 1,
                        "java"=> *m.entry("java").or_default()        += 1,
                        "cs"  => *m.entry("csharp").or_default()      += 1,
                        "cpp" | "cc" | "cxx" => *m.entry("c++").or_default() += 1,
                        "c"   => *m.entry("c").or_default()           += 1,
                        _ => {}
                    }
                }
            }
        }
        m
    };
    if let Some((&lang, _)) = ext_counts.iter().max_by_key(|(_, &v)| v) {
        return lang;
    }
    let has_any_files = std::fs::read_dir(".")
        .map(|d| d.flatten().any(|e| e.path().is_file()))
        .unwrap_or(false);
    if has_any_files { "general" } else { "" }
}

pub(super) fn generate_zap_md_template(project_type: &str) -> String {
    let (build_cmd, test_cmd, lint_cmd) = match project_type {
        "rust"       => ("cargo build",       "cargo test",      "cargo clippy"),
        "go"         => ("go build ./...",    "go test ./...",   "golangci-lint run"),
        "typescript" => ("npm run build",     "npm test",        "npm run lint"),
        "javascript" => ("npm run build",     "npm test",        "npm run lint"),
        "python"     => ("pip install -e .",  "pytest",          "ruff check ."),
        "java"       => ("mvn compile",       "mvn test",        "mvn checkstyle:check"),
        "kotlin"     => ("./gradlew build",   "./gradlew test",  "./gradlew lint"),
        "swift"      => ("swift build",       "swift test",      "swiftlint"),
        "c++"        => ("cmake --build .",   "ctest",           "clang-tidy"),
        "c"          => ("make",              "make test",       "clang-tidy"),
        _            => ("make",              "make test",       "make lint"),
    };
    format!(
        r#"# Project Instructions

## Overview
<!-- Describe what this project does in 1-3 sentences. -->

## Build & Test
```
{build}
{test}
{lint}
```

## Code Style
<!-- List any conventions zap must follow: naming, formatting, imports, etc. -->

## Architecture
<!-- Briefly describe the module layout and main data-flow so zap has context. -->

## Important Files
<!-- List key files or directories zap should know about. -->

## Do Not Touch
<!-- List files, directories, or patterns that must not be modified without explicit approval. -->

## Notes
<!-- Anything else zap should know. -->
"#,
        build = build_cmd, test = test_cmd, lint = lint_cmd,
    )
}
