use std::collections::HashMap;

use super::app::{App, MsgRole, ToolDone, UiBlock, UiMessage, UiToolCall};

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
    let skill_note = {
        let always_on_count = crate::skill_manager::always_on_skills(&session.skills).len();
        let practice_count = session.skills.iter()
            .filter(|s| s.category == crate::skill_manager::SkillCategory::Practice).count();
        let domain_count = session.skills.iter()
            .filter(|s| s.category == crate::skill_manager::SkillCategory::Domain).count();
        if session.skills.is_empty() {
            String::new()
        } else {
            format!("  ·  {} skills ({} core · {} practice · {} domain)",
                session.skills.len(), always_on_count, practice_count, domain_count)
        }
    };
    app.messages.push(UiMessage {
        role: MsgRole::Assistant,
        blocks: vec![UiBlock::Text(format!(
            "Ready. {} tools loaded{}.",
            session.tool_count, skill_note
        ))],
    });

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
