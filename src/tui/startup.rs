use std::collections::HashMap;

use super::app::{App, MsgRole, ProviderKind, ProviderPickerState, ProviderEntry, ToolDone, UiBlock, UiMessage, UiToolCall};

/// Replay the previous session's conversation into the app for display at startup.
pub(super) fn replay_last_session_into_app(app: &mut App, session: &crate::session::Session) {
    let has_last_banner = session.startup_notices.iter().any(|n| n.starts_with("↩ Last:"));
    if !has_last_banner { return; }

    if let Ok(sessions) = session.store.recent_sessions(2) {
        if let Some((prev_id, _goal, _model, _created)) = sessions.get(1) {
            let prev_id = *prev_id;
            if let Ok(Some(json)) = session.store.load_messages(prev_id) {
                if let Ok(msgs) = serde_json::from_str::<Vec<crate::llm_client::Message>>(&json) {
                    let tool_results: HashMap<&str, &str> = msgs.iter()
                        .flat_map(|m| m.content.iter())
                        .filter_map(|b| match b {
                            crate::llm_client::ContentBlock::ToolResult { tool_use_id, content } =>
                                Some((tool_use_id.as_str(), content.as_str())),
                            _ => None,
                        })
                        .collect();

                    for msg in &msgs {
                        if msg.content.iter().any(|b| matches!(b, crate::llm_client::ContentBlock::ToolResult { .. })) {
                            continue;
                        }
                        let role = match msg.role.as_str() {
                            "user" => MsgRole::User,
                            _ => MsgRole::Assistant,
                        };
                        let blocks: Vec<UiBlock> = build_ui_blocks(&msg.content, &tool_results);
                        if !blocks.is_empty() {
                            app.messages.push(UiMessage { role, blocks });
                        }
                    }
                    app.messages.push(UiMessage {
                        role: MsgRole::Assistant,
                        blocks: vec![UiBlock::Text(format!("─── end of session #{prev_id} ───"))],
                    });
                    app.auto_scroll = true;
                }
            }
        }
    }
}

/// Build and push welcome message + drain startup notices into the app.
pub(super) fn push_startup_messages(app: &mut App, session: &mut crate::session::Session) {
    for notice in session.startup_notices.drain(..) {
        app.messages.push(UiMessage {
            role: MsgRole::Assistant,
            blocks: vec![UiBlock::Text(notice)],
        });
    }

    let not_indexed = crate::project::load_project_meta()
        .map(|m| !m.indexed)
        .unwrap_or(true);
    if not_indexed {
        app.messages.push(UiMessage {
            role: MsgRole::Assistant,
            blocks: vec![UiBlock::Text(
                "Tip: run /init to index this project for faster code navigation. \
                 Indexing is 100% local — your code is parsed by tree-sitter and stored \
                 in .zap/code.db (SQLite) on your machine. Nothing is sent to any server \
                 or cloud during indexing. Only the messages you type go to the LLM."
                    .to_string(),
            )],
        });
    }
}

/// Load a historical session's messages into the TUI view and session state.
pub(super) fn load_session_into_app(
    app: &mut App,
    session: &mut crate::session::Session,
    sid: i64,
    goal: String,
) {
    match session.store.load_messages(sid) {
        Ok(Some(json)) => {
            match serde_json::from_str::<Vec<crate::llm_client::Message>>(&json) {
                Ok(msgs) => {
                    let count = msgs.len();
                    let turns = msgs.iter().filter(|m| m.role == "user").count();
                    let tool_results: HashMap<&str, &str> = msgs.iter()
                        .flat_map(|m| m.content.iter())
                        .filter_map(|b| match b {
                            crate::llm_client::ContentBlock::ToolResult { tool_use_id, content } =>
                                Some((tool_use_id.as_str(), content.as_str())),
                            _ => None,
                        })
                        .collect();

                    app.messages.clear();
                    for msg in &msgs {
                        if msg.content.iter().any(|b| matches!(b, crate::llm_client::ContentBlock::ToolResult { .. })) {
                            continue;
                        }
                        let role = match msg.role.as_str() {
                            "user" => MsgRole::User,
                            _ => MsgRole::Assistant,
                        };
                        let blocks: Vec<UiBlock> = build_ui_blocks(&msg.content, &tool_results);
                        if !blocks.is_empty() {
                            app.messages.push(UiMessage { role, blocks });
                        }
                    }

                    session.messages   = msgs;
                    session.turn_count = turns;
                    session.session_id = sid;
                    let files_note = crate::project::session_log_files(sid)
                        .map(|f| format!("\nFiles: {}", f))
                        .unwrap_or_default();
                    app.messages.push(UiMessage {
                        role: MsgRole::Assistant,
                        blocks: vec![UiBlock::Text(format!(
                            "Resumed session #{sid} — {turns} turns, {count} messages.{files_note}"
                        ))],
                    });
                    app.auto_scroll = true;
                }
                Err(e) => app.error = Some(format!("session parse error: {e}")),
            }
        }
        Ok(None) => {
            app.messages.clear();
            let files_note = crate::project::session_log_files(sid)
                .map(|f| format!("\nFiles: {}", f))
                .unwrap_or_default();
            app.messages.push(UiMessage {
                role: MsgRole::Assistant,
                blocks: vec![UiBlock::Text(format!(
                    "Session #{sid} — no conversation saved.\nGoal: {goal}{files_note}\n\nYou can continue from here."
                ))],
            });
            session.session_id = sid;
            app.auto_scroll = true;
        }
        Err(e) => app.error = Some(format!("load session: {e}")),
    }
}

fn build_ui_blocks(
    content: &[crate::llm_client::ContentBlock],
    tool_results: &HashMap<&str, &str>,
) -> Vec<UiBlock> {
    content.iter().filter_map(|b| match b {
        crate::llm_client::ContentBlock::Text { text } => {
            Some(UiBlock::Text(text.clone()))
        }
        crate::llm_client::ContentBlock::ToolUse { id, name, input } => {
            let input_str = serde_json::to_string(input).unwrap_or_default();
            let label = if input_str.chars().count() > 100 {
                format!("{}…", input_str.chars().take(97).collect::<String>())
            } else {
                input_str
            };
            let result = tool_results.get(id.as_str()).map(|content| {
                let preview = if content.chars().count() > 200 {
                    format!("{}…", content.chars().take(197).collect::<String>())
                } else {
                    (*content).to_string()
                };
                ToolDone { elapsed_ms: 0, success: true, preview }
            });
            Some(UiBlock::Tool(UiToolCall {
                id: id.clone(),
                name: name.clone(),
                label,
                result,
            }))
        }
        crate::llm_client::ContentBlock::Thinking { thinking, .. } => {
            Some(UiBlock::Thinking { char_count: thinking.chars().count() })
        }
        crate::llm_client::ContentBlock::Reasoning { content } => {
            Some(UiBlock::Text(format!("[Reasoning]\n{}", content)))
        }
        _ => None,
    }).collect()
}

/// On first launch with no provider configured, auto-open the provider picker.
pub(super) fn maybe_open_onboarding_picker(app: &mut App, config: &crate::config::Config) {
    let no_provider_configured = config.all_providers.is_empty()
        && std::env::var("AGENT_API_KEY").is_err()
        && std::env::var("ANTHROPIC_API_KEY").is_err()
        && std::env::var("OPENAI_API_KEY").is_err()
        && std::env::var("GOOGLE_API_KEY").is_err();

    if !no_provider_configured { return; }

    let gemini_ready = crate::llm_client::auth::check_gcloud_adc().is_some()
        || crate::llm_client::auth::check_google_api_key_env().is_some();
    let claude_code_ready = crate::llm_client::auth::check_claude_code().is_some();

    let entries: Vec<ProviderEntry> = vec![
        ProviderEntry { slug: "lm_studio".into(),  name: "LM Studio".into(),                  hint: "local · OpenAI-compatible".into(),                    kind: ProviderKind::OpenAi,    models: vec!["gemma-4-e4b-it".into(), "qwen2.5-coder-7b-instruct".into(), "mistral-7b-instruct".into(), "Other…".into()],              base_url: Some("http://localhost:1234/v1/chat/completions".into()),                                    needs_key: false, coming_soon: false, auth_header: None,                       ready: true },
        ProviderEntry { slug: "ollama".into(),     name: "Ollama".into(),                     hint: "local · OpenAI-compatible".into(),                    kind: ProviderKind::OpenAi,    models: vec!["llama3.2".into(), "llama3.1:70b".into(), "codellama".into(), "qwen2.5-coder".into(), "Other…".into()],              base_url: Some("http://localhost:11434/v1/chat/completions".into()),                                   needs_key: false, coming_soon: false, auth_header: None,                       ready: true },
        ProviderEntry { slug: "anthropic".into(),  name: "Anthropic".into(),                  hint: "claude-sonnet-4-6 / claude-opus-4-7".into(),          kind: ProviderKind::Anthropic, models: vec!["claude-sonnet-4-6".into(), "claude-opus-4-7".into(), "claude-haiku-4-5".into(), "Other…".into()],              base_url: None,                                                                                        needs_key: true,  coming_soon: false, auth_header: None,                       ready: false },
        ProviderEntry { slug: "claude_code".into(),name: "Claude Code (Pro/Max API)".into(),  hint: if claude_code_ready { "claude-sonnet-4-6 / claude-opus-4-7 · via claude CLI".into() } else { "requires claude CLI · Pro/Max plan".into() }, kind: ProviderKind::Anthropic, models: vec!["claude-sonnet-4-6".into(), "claude-opus-4-7".into()],                                                           base_url: None,                                                                                        needs_key: false, coming_soon: !claude_code_ready, auth_header: None, ready: claude_code_ready },
        ProviderEntry { slug: "openai".into(),     name: "OpenAI".into(),                     hint: "gpt-4o / gpt-4o-mini / o3".into(),                    kind: ProviderKind::OpenAi,    models: vec!["gpt-4o".into(), "gpt-4o-mini".into(), "o3".into(), "o4-mini".into(), "Other…".into()],                            base_url: None,                                                                                        needs_key: true,  coming_soon: false, auth_header: None,                       ready: false },
        ProviderEntry { slug: "gemini".into(),     name: "Google Gemini".into(),              hint: "get API key at aistudio.google.com/apikey".into(),            kind: ProviderKind::OpenAi,    models: vec!["gemini-2.0-flash".into(), "gemini-2.5-pro".into(), "gemini-2.5-flash".into(), "Other…".into()],                    base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".into()),    needs_key: true,  coming_soon: false, auth_header: None,                       ready: gemini_ready },
        ProviderEntry { slug: "deepseek".into(),   name: "DeepSeek".into(),                   hint: "deepseek-v4-pro / deepseek-v4-flash".into(),          kind: ProviderKind::OpenAi,    models: vec!["deepseek-v4-pro".into(), "deepseek-v4-flash".into(), "deepseek-chat".into(), "deepseek-reasoner".into(), "Other…".into()], base_url: Some("https://api.deepseek.com/v1/chat/completions".into()),                              needs_key: true,  coming_soon: false, auth_header: None,                       ready: false },
        ProviderEntry { slug: "groq".into(),       name: "Groq".into(),                       hint: "llama-3.3-70b · fastest inference".into(),            kind: ProviderKind::OpenAi,    models: vec!["llama-3.3-70b-versatile".into(), "llama-3.1-8b-instant".into(), "mixtral-8x7b-32768".into(), "Other…".into()],       base_url: Some("https://api.groq.com/openai/v1/chat/completions".into()),                             needs_key: true,  coming_soon: false, auth_header: None,                       ready: false },
        ProviderEntry { slug: "mistral".into(),    name: "Mistral".into(),                    hint: "mistral-large / codestral".into(),                    kind: ProviderKind::OpenAi,    models: vec!["mistral-large-latest".into(), "codestral-latest".into(), "mistral-small-latest".into(), "Other…".into()],           base_url: Some("https://api.mistral.ai/v1/chat/completions".into()),                                  needs_key: true,  coming_soon: false, auth_header: None,                       ready: false },
        ProviderEntry { slug: "xai".into(),        name: "xAI (Grok)".into(),                 hint: "grok-3 / grok-3-mini".into(),                         kind: ProviderKind::OpenAi,    models: vec!["grok-3".into(), "grok-3-mini".into(), "grok-2".into(), "Other…".into()],                                          base_url: Some("https://api.x.ai/v1/chat/completions".into()),                                        needs_key: true,  coming_soon: false, auth_header: None,                       ready: false },
        ProviderEntry { slug: "together".into(),   name: "Together AI".into(),                hint: "Llama / Qwen / Mistral open models".into(),           kind: ProviderKind::OpenAi,    models: vec!["meta-llama/Llama-3-70b-chat-hf".into(), "Qwen/Qwen2.5-72B-Instruct-Turbo".into(), "Other…".into()],              base_url: Some("https://api.together.xyz/v1/chat/completions".into()),                               needs_key: true,  coming_soon: false, auth_header: None,                       ready: false },
        ProviderEntry { slug: "perplexity".into(), name: "Perplexity".into(),                 hint: "sonar-pro · web-grounded answers".into(),             kind: ProviderKind::OpenAi,    models: vec!["sonar-pro".into(), "sonar".into(), "sonar-reasoning".into(), "Other…".into()],                                    base_url: Some("https://api.perplexity.ai/chat/completions".into()),                                  needs_key: true,  coming_soon: false, auth_header: None,                       ready: false },
        ProviderEntry { slug: "cohere".into(),     name: "Cohere".into(),                     hint: "command-r-plus".into(),                               kind: ProviderKind::OpenAi,    models: vec!["command-r-plus".into(), "command-r".into(), "Other…".into()],                                                      base_url: Some("https://api.cohere.ai/compatibility/v1/chat/completions".into()),                    needs_key: true,  coming_soon: false, auth_header: None,                       ready: false },
        ProviderEntry { slug: "custom".into(),     name: "Custom (OpenAI-compatible)".into(), hint: "any OpenAI-compatible endpoint".into(),               kind: ProviderKind::OpenAi,    models: vec!["Other…".into()],                                                                                                     base_url: None,                                                                                        needs_key: false, coming_soon: false, auth_header: None,                       ready: false },
    ];

    let selected = if claude_code_ready {
        entries.iter().position(|e| e.slug == "claude_code").unwrap_or(0)
    } else if gemini_ready {
        entries.iter().position(|e| e.slug == "gemini").unwrap_or(0)
    } else {
        0
    };

    app.provider_picker = Some(ProviderPickerState { entries, selected, is_onboarding: true });

    app.messages.push(UiMessage {
        role: MsgRole::Assistant,
        blocks: vec![UiBlock::Text(
            "Welcome to zap! No LLM provider is configured yet.\n\
             Providers marked  ✓ ready  were auto-detected from your environment.\n\
             Select one above, then start chatting. You can switch anytime with /provider."
                .to_string(),
        )],
    });
    app.auto_scroll = true;
}
