use std::borrow::Cow;
use crate::llm_client::{ContentBlock, Message};

/// Best-effort context window size for known model families.
pub fn model_context_limit(model: &str) -> usize {
    let m = model.to_lowercase();
    if m.contains("claude")                                        { 200_000 }
    else if m.contains("gemini-1.5") || m.contains("gemini-2")    { 1_000_000 }
    else if m.contains("gemini")
         || m.contains("gpt-4o") || m.contains("gpt-4-turbo")
         || m.contains("o3") || m.contains("o4")                   { 128_000 }
    else if m.contains("gpt-3.5")                                  { 16_385 }
    else if m.contains("deepseek")                                 { 64_000 }
    else                                                            { 32_768 }
}

/// Renders a 10-block ASCII progress bar: `[████████░░] 80%`
pub(super) fn ctx_bar(pct: u8) -> String {
    let filled = (pct as usize).min(100) * 10 / 100;
    let bar: String = (0..10).map(|i| if i < filled { '█' } else { '░' }).collect();
    format!("[{}] {}%", bar, pct)
}

/// Return the tool definitions to send with a single LLM call.
///
/// Anthropic: always send everything — prompt caching makes repeated tool
/// schemas essentially free from turn 2 onward.
///
/// OpenAI-compatible: smaller models benefit from a tighter tool set.
/// We gate `web_fetch` / `web_search` behind a keyword check so they
/// don't bloat every request.
pub(super) fn select_tools_for_turn<'a>(
    all: &'a [serde_json::Value],
    user_input: &str,
    config: &crate::config::Config,
    messages: &[Message],
) -> Cow<'a, [serde_json::Value]> {
    use crate::config::Provider;

    if matches!(config.provider, Provider::Anthropic) {
        return Cow::Borrowed(all);
    }

    let lower = user_input.to_lowercase();
    let wants_web_now = ["http://", "https://", "url", " web ", "website",
                         "fetch ", "curl ", "download", "browse", "docs",
                         "documentation", "web_fetch", "web_search"]
        .iter().any(|kw| lower.contains(kw));

    let web_used = messages.iter().any(|m| {
        m.content.iter().any(|b| matches!(
            b,
            ContentBlock::ToolUse { name, .. }
            if name == "web_fetch" || name == "web_search"
        ))
    });

    if wants_web_now || web_used {
        return Cow::Borrowed(all);
    }

    let filtered: Vec<serde_json::Value> = all.iter()
        .filter(|def| !matches!(
            def["name"].as_str().unwrap_or(""),
            "web_fetch" | "web_search"
        ))
        .cloned()
        .collect();
    Cow::Owned(filtered)
}

/// Build the trimmed message slice to send for a non-casual turn.
///
/// 1. **Sliding window** — only the last `ZAP_HISTORY_WINDOW` real user turns
///    (default 8) are included, bounding token cost.
/// 2. **Drop summary** — when turns fall off the window, a synthetic summary
///    message pair is prepended so the LLM retains key context from early turns.
/// 3. **Tool-result pruning** — ToolResult blocks outside the last 2 complete
///    exchanges are replaced with a stub + 150-char preview.
pub(super) fn windowed_history(messages: &[Message]) -> Vec<Message> {
    let window: usize = std::env::var("ZAP_HISTORY_WINDOW")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8);

    let real_turn_indices: Vec<usize> = messages.iter().enumerate()
        .filter(|(_, m)| {
            m.role == "user"
                && m.content.first()
                    .is_some_and(|b| matches!(b, ContentBlock::Text { .. }))
        })
        .map(|(i, _)| i)
        .collect();

    let start = if real_turn_indices.len() > window {
        real_turn_indices[real_turn_indices.len() - window]
    } else {
        0
    };

    let prune_before = if real_turn_indices.len() > 2 {
        real_turn_indices[real_turn_indices.len() - 2]
    } else {
        0
    };

    const PRUNE_THRESHOLD: usize = 300;
    const PRUNE_PREVIEW:   usize = 150;

    let windowed: Vec<Message> = messages[start..].iter().enumerate()
        .map(|(rel_i, msg)| {
            let abs_i = start + rel_i;
            if abs_i < prune_before {
                let pruned: Vec<ContentBlock> = msg.content.iter()
                    .map(|block| match block {
                        ContentBlock::ToolResult { tool_use_id, content }
                            if content.len() > PRUNE_THRESHOLD =>
                        {
                            let preview: String = content.chars().take(PRUNE_PREVIEW).collect();
                            ContentBlock::ToolResult {
                                tool_use_id: tool_use_id.clone(),
                                content: format!("[pruned — {} chars]\n{}", content.len(), preview),
                            }
                        }
                        other => other.clone(),
                    })
                    .collect();
                Message { role: msg.role.clone(), content: pruned }
            } else {
                msg.clone()
            }
        })
        .collect();

    if start == 0 {
        return windowed;
    }

    // Turns were dropped — prepend a synthetic summary so the LLM knows what it missed.
    let summary = build_drop_summary(&messages[..start]);
    let mut result = Vec::with_capacity(windowed.len() + 2);
    result.push(Message::user_text(summary));
    result.push(Message {
        role:    "assistant".to_string(),
        content: vec![ContentBlock::Text {
            text: "Understood — I have the context from the earlier turns.".to_string(),
        }],
    });
    result.extend(windowed);
    result
}

/// Build a concise text summary of dropped turns without calling the LLM.
///
/// Shows up to 5 of the most recent dropped turns (oldest first), with short
/// previews of the user request and assistant response, plus tool names used.
fn build_drop_summary(dropped: &[Message]) -> String {
    const USER_PREVIEW: usize = 200;
    const ASST_PREVIEW: usize = 250;
    const MAX_TOOLS:    usize = 5;
    const MAX_TURNS:    usize = 5;

    let real_positions: Vec<usize> = dropped.iter().enumerate()
        .filter(|(_, m)| {
            m.role == "user"
                && m.content.first()
                    .is_some_and(|b| matches!(b, ContentBlock::Text { .. }))
        })
        .map(|(i, _)| i)
        .collect();

    let total = real_positions.len();
    let show_start = total.saturating_sub(MAX_TURNS);

    let header = if total > MAX_TURNS {
        format!(
            "[History summary — {} earlier turns condensed, showing last {}]",
            total, MAX_TURNS
        )
    } else {
        format!(
            "[History summary — {} earlier turn{} condensed]",
            total, if total == 1 { "" } else { "s" }
        )
    };

    let mut parts = vec![header];

    for (idx, &pos) in real_positions[show_start..].iter().enumerate() {
        let turn_num = show_start + idx + 1;
        let next_pos = real_positions.get(show_start + idx + 1).copied().unwrap_or(dropped.len());

        let user_text = dropped[pos].content.iter()
            .find_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
            .unwrap_or("");
        let user_preview: String = user_text.chars().take(USER_PREVIEW).collect();
        let user_suffix = if user_text.chars().count() > USER_PREVIEW { "…" } else { "" };

        let mut asst_preview = String::new();
        let mut tool_names: Vec<String> = Vec::new();

        for msg in &dropped[pos + 1..next_pos] {
            for block in &msg.content {
                match block {
                    ContentBlock::Text { text } if msg.role == "assistant" && asst_preview.is_empty() => {
                        asst_preview = text.chars().take(ASST_PREVIEW).collect();
                        if text.chars().count() > ASST_PREVIEW { asst_preview.push('…'); }
                    }
                    ContentBlock::ToolUse { name, input, .. } if tool_names.len() < MAX_TOOLS => {
                        let file_hint = input.get("path")
                            .or_else(|| input.get("file_path"))
                            .and_then(|v| v.as_str())
                            .map(|p| {
                                // Trim to basename to keep it short
                                std::path::Path::new(p)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(p)
                                    .to_string()
                            })
                            .map(|f| format!("({})", f))
                            .unwrap_or_default();
                        tool_names.push(format!("{}{}", name, file_hint));
                    }
                    _ => {}
                }
            }
        }

        let mut entry = format!("Turn {}: {}{}", turn_num, user_preview, user_suffix);
        if !asst_preview.is_empty() {
            entry.push_str(&format!("\n  → {}", asst_preview));
        }
        if !tool_names.is_empty() {
            entry.push_str(&format!("\n  [{}]", tool_names.join(", ")));
        }
        parts.push(entry);
    }

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::{ctx_bar, model_context_limit, windowed_history};
    use crate::llm_client::{ContentBlock, Message};

    fn user_text(t: &str) -> Message {
        Message::user_text(t)
    }
    fn tool_result(id: &str, content: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: vec![ContentBlock::ToolResult {
                tool_use_id: id.to_string(),
                content: content.to_string(),
            }],
        }
    }

    // ── model_context_limit ───────────────────────────────────────────────────

    #[test]
    fn claude_gets_200k() {
        assert_eq!(model_context_limit("claude-3-5-sonnet"), 200_000);
    }

    #[test]
    fn gpt35_gets_16k() {
        assert_eq!(model_context_limit("gpt-3.5-turbo"), 16_385);
    }

    #[test]
    fn unknown_model_gets_default() {
        assert_eq!(model_context_limit("llama-3-70b"), 32_768);
    }

    // ── ctx_bar ───────────────────────────────────────────────────────────────

    #[test]
    fn ctx_bar_zero() {
        assert_eq!(ctx_bar(0), "[░░░░░░░░░░] 0%");
    }

    #[test]
    fn ctx_bar_full() {
        assert_eq!(ctx_bar(100), "[██████████] 100%");
    }

    #[test]
    fn ctx_bar_half() {
        assert_eq!(ctx_bar(50), "[█████░░░░░] 50%");
    }

    // ── windowed_history ──────────────────────────────────────────────────────

    #[test]
    fn empty_history_returns_empty() {
        assert!(windowed_history(&[]).is_empty());
    }

    #[test]
    fn small_history_returned_intact() {
        let msgs = vec![user_text("hi"), user_text("hello")];
        assert_eq!(windowed_history(&msgs).len(), 2);
    }

    #[test]
    fn tool_results_pruned_outside_window() {
        let big = "x".repeat(400);
        let mut msgs = vec![
            user_text("turn1"),
            tool_result("id1", &big),
            user_text("turn2"),
            tool_result("id2", &big),
            user_text("turn3"),
        ];
        let history = windowed_history(&msgs);
        let pruned = history.iter().any(|m| {
            m.content.iter().any(|b| {
                if let ContentBlock::ToolResult { content, .. } = b {
                    content.starts_with("[pruned")
                } else { false }
            })
        });
        assert!(pruned, "oversized tool result should be pruned");
        let last_tool = history.iter().rev().find(|m| {
            m.content.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }))
        });
        if let Some(m) = last_tool {
            let content = m.content.iter().find_map(|b| {
                if let ContentBlock::ToolResult { content, .. } = b { Some(content.clone()) } else { None }
            }).unwrap();
            assert!(!content.starts_with("[pruned"), "recent tool result must not be pruned");
        }
        drop(msgs.pop());
    }

    #[test]
    fn pruned_tool_result_includes_preview() {
        let big = "abcdefghij".repeat(50); // 500 chars
        let msgs = vec![
            user_text("turn1"),
            tool_result("id1", &big),
            user_text("turn2"),
            tool_result("id2", "short"),
            user_text("turn3"),
        ];
        let history = windowed_history(&msgs);
        // Find the pruned tool result
        let pruned_content = history.iter().find_map(|m| {
            m.content.iter().find_map(|b| {
                if let ContentBlock::ToolResult { content, .. } = b {
                    if content.starts_with("[pruned") { Some(content.clone()) } else { None }
                } else { None }
            })
        });
        let content = pruned_content.expect("pruned result should exist");
        assert!(content.contains("500 chars"), "should report original length");
        // Preview of first 150 chars of "abcdefghij" * 50 = "abcdefghijabcdefghij..."
        assert!(content.contains("abcdefghij"), "should include content preview");
    }

    #[test]
    fn no_drop_summary_within_window() {
        // 5 real turns — all fit inside default window of 8, no summary injected
        let msgs: Vec<Message> = (0..5).map(|i| user_text(&format!("turn {}", i))).collect();
        let history = windowed_history(&msgs);
        let has_summary = history.iter().any(|m| {
            m.content.iter().any(|b| {
                if let ContentBlock::Text { text } = b {
                    text.contains("[History summary")
                } else { false }
            })
        });
        assert!(!has_summary, "no summary when all turns fit in window");
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn drop_summary_injected_when_window_slides() {
        fn make_exchange(user: &str, asst: &str) -> Vec<Message> {
            vec![
                user_text(user),
                Message {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::Text { text: asst.to_string() }],
                },
            ]
        }

        // Build 10 exchanges (>8 window default) → turns 1-2 drop off
        let mut msgs: Vec<Message> = Vec::new();
        for i in 1..=10 {
            msgs.extend(make_exchange(
                &format!("user message {}", i),
                &format!("assistant reply {}", i),
            ));
        }

        let history = windowed_history(&msgs);

        // First message should be the summary
        let first = &history[0];
        let first_text = first.content.iter().find_map(|b| {
            if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
        }).unwrap_or("");
        assert!(first_text.contains("[History summary"), "first message should be summary");
        assert!(first_text.contains("Turn 1"), "summary should reference dropped turns");

        // Second message should be the synthetic assistant ack
        assert_eq!(history[1].role, "assistant");

        // The windowed portion should follow (8 real turns × 2 messages + 2 synthetic = 18 total)
        assert_eq!(history.len(), 2 + 16, "2 synthetic + 8 turns × 2 messages");
    }
}
