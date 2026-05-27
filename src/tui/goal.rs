use super::app::{App, GoalState, MsgRole, StreamingBlock, UiBlock, UiMessage};

pub(super) fn handle_goal_command(app: &mut App, arg: &str) {
    let arg = arg.trim();
    if arg.is_empty() || arg == "status" {
        let text = if let Some(ref gs) = app.goal_state {
            format!(
                "**Goal active** — {}/{} turns  {}s elapsed\n\nCondition: {}\n\n`/goal stop` to cancel",
                gs.turns_done, gs.max_turns,
                gs.started_at.elapsed().as_secs(),
                gs.condition,
            )
        } else {
            "No active goal.\n\nUsage: `/goal <condition>` — zap keeps working turn-by-turn until the goal is met or `--max N` turns (default 20) are exhausted.\n\nExample: `/goal add unit tests for the auth module`".to_string()
        };
        app.messages.push(UiMessage { role: MsgRole::Assistant, blocks: vec![UiBlock::Text(text)] });
        app.auto_scroll = true;
        return;
    }
    if arg == "stop" || arg == "cancel" {
        app.goal_state = None;
        app.messages.push(UiMessage {
            role: MsgRole::Assistant,
            blocks: vec![UiBlock::Text("Goal stopped.".to_string())],
        });
        app.auto_scroll = true;
        return;
    }
    // Parse optional --max N
    let (condition, max_turns) = if let Some(idx) = arg.find("--max") {
        let cond = arg[..idx].trim().to_string();
        let rest = arg[idx + 5..].trim();
        let n: usize = rest.split_whitespace().next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(20);
        (cond, n)
    } else {
        (arg.to_string(), 20)
    };

    app.goal_state = Some(GoalState {
        condition: condition.clone(),
        max_turns,
        turns_done: 0,
        started_at: std::time::Instant::now(),
    });
    let first = format!(
        "[Goal 1/{max}] {cond}\n\nWhen the goal is fully complete, end your response with exactly: ✓ DONE",
        max = max_turns, cond = condition
    );
    app.messages.push(UiMessage { role: MsgRole::User, blocks: vec![UiBlock::Text(first.clone())] });
    app.pending_input = Some(first);
    app.auto_scroll = true;
}

/// Check the last assistant message for the ✓ DONE completion marker.
pub(super) fn goal_response_is_done(app: &App) -> bool {
    for msg in app.messages.iter().rev() {
        if matches!(msg.role, MsgRole::Assistant) {
            for block in &msg.blocks {
                if let UiBlock::Text(text) = block {
                    if text.contains("✓ DONE") || text.contains("✓DONE")
                        || text.to_lowercase().contains("✓ done")
                    {
                        return true;
                    }
                }
            }
            break;
        }
    }
    false
}

/// Walk completed messages (newest first) to find the next tool to expand.
/// Returns None when all tools are already expanded (signals caller to collapse all).
pub(super) fn next_tool_id_to_expand(app: &App) -> Option<String> {
    let mut all_ids: Vec<String> = Vec::new();
    for msg in app.messages.iter().rev() {
        for block in msg.blocks.iter().rev() {
            if let UiBlock::Tool(tc) = block {
                if tc.result.is_some() {
                    all_ids.push(tc.id.clone());
                }
            }
        }
    }
    for sb in app.streaming_blocks.iter().rev() {
        if let StreamingBlock::Tool(tc) = sb {
            if tc.result.is_some() {
                all_ids.push(tc.id.clone());
            }
        }
    }

    if all_ids.is_empty() { return None; }

    if let Some(id) = all_ids.iter().find(|id| !app.expanded_tools.contains(*id)) {
        return Some(id.clone());
    }

    None
}
