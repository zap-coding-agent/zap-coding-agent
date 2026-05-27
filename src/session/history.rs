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
/// 2. **Tool-result pruning** — ToolResult blocks outside the last 2 complete
///    exchanges are replaced with a one-line stub to avoid inflating the prompt.
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

    messages[start..].iter().enumerate()
        .map(|(rel_i, msg)| {
            let abs_i = start + rel_i;
            if abs_i < prune_before {
                let pruned: Vec<ContentBlock> = msg.content.iter()
                    .map(|block| match block {
                        ContentBlock::ToolResult { tool_use_id, content }
                            if content.len() > PRUNE_THRESHOLD =>
                        {
                            ContentBlock::ToolResult {
                                tool_use_id: tool_use_id.clone(),
                                content: format!("[pruned — {} chars]", content.len()),
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
        .collect()
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
        // Build 3 real user turns with an oversized tool result before the window.
        let big = "x".repeat(400);
        let mut msgs = vec![
            user_text("turn1"),
            tool_result("id1", &big),
            user_text("turn2"),
            tool_result("id2", &big),
            user_text("turn3"),
        ];
        // With window=8 and 3 real turns, prune_before = real_turn_indices[1] = 2.
        // The tool result at index 1 should be pruned.
        let history = windowed_history(&msgs);
        let pruned = history.iter().any(|m| {
            m.content.iter().any(|b| {
                if let ContentBlock::ToolResult { content, .. } = b {
                    content.starts_with("[pruned")
                } else { false }
            })
        });
        assert!(pruned, "oversized tool result should be pruned");
        // The last tool result (within window) must NOT be pruned.
        let last_tool = history.iter().rev().find(|m| {
            m.content.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }))
        });
        if let Some(m) = last_tool {
            let content = m.content.iter().find_map(|b| {
                if let ContentBlock::ToolResult { content, .. } = b { Some(content.clone()) } else { None }
            }).unwrap();
            assert!(!content.starts_with("[pruned"), "recent tool result must not be pruned");
        }
        drop(msgs.pop()); // suppress unused warning
    }
}
