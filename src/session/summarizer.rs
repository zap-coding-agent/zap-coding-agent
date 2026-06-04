use crate::llm_client::{ContentBlock, Message};
use super::Session;

impl Session {
    /// Called once per user turn before the LLM loop.
    ///
    /// Detects whether the sliding window has advanced since the last turn (i.e. new turns
    /// have dropped off the front). For each newly-dropped batch, calls the LLM to produce
    /// a concise bullet-point summary and appends it to `self.dropped_summary`.
    ///
    /// Falls back to a text-only summary if the LLM call fails, so a slow or unavailable
    /// model never blocks the turn.
    pub async fn maybe_summarize_dropped_turns(&mut self) {
        let window: usize = std::env::var("ZAP_HISTORY_WINDOW")
            .ok().and_then(|s| s.parse().ok()).unwrap_or(8);

        let real_turn_indices: Vec<usize> = self.messages.iter().enumerate()
            .filter(|(_, m)| {
                m.role == "user"
                    && m.content.first()
                        .is_some_and(|b| matches!(b, ContentBlock::Text { .. }))
            })
            .map(|(i, _)| i)
            .collect();

        if real_turn_indices.len() <= window {
            return; // all turns still fit — nothing to drop
        }

        let current_start = real_turn_indices[real_turn_indices.len() - window];
        if current_start <= self.last_window_start {
            return; // window hasn't moved since last turn
        }

        let newly_dropped = self.messages[self.last_window_start..current_start].to_vec();

        let new_section = match self.call_drop_summarizer(&newly_dropped).await {
            Ok(s)  => s,
            Err(e) => {
                crate::log::write("WARN", &format!("drop-summary LLM call failed: {e}"));
                Self::text_drop_summary(&newly_dropped)
            }
        };

        if !self.dropped_summary.is_empty() {
            self.dropped_summary.push_str("\n\n");
        }
        self.dropped_summary.push_str(&new_section);
        self.last_window_start = current_start;

        // Re-compress the running summary if it exceeds 6 000 chars (~1 500 tokens).
        // Clone first to release the borrow before the &mut self call.
        if self.dropped_summary.len() > 6_000 {
            let current = self.dropped_summary.clone();
            let recompress_msg = vec![Message::user_text(format!(
                "Compress this running session summary into ≤ 400 words while \
                 preserving all key facts, user preferences, files modified, and \
                 decisions made:\n\n{}",
                current
            ))];
            if let Ok(compressed) = self.call_drop_summarizer(&recompress_msg).await {
                self.dropped_summary = format!("[Compressed history]\n{}", compressed);
            }
            // If re-compression also fails, leave the existing summary as-is.
        }
    }

    /// Call the LLM with a focused summarization prompt for a slice of dropped messages.
    ///
    /// Takes `&mut self` (not `&self`) so the future is `Send` — `&Session` is `!Send`
    /// because rusqlite's connection cache uses `RefCell` (which is `!Sync`).
    async fn call_drop_summarizer(
        &mut self,
        messages: &[Message],
    ) -> anyhow::Result<String> {
        let mut prompt = messages.to_vec();
        prompt.push(Message::user_text(
            "Summarise the conversation turns above into concise bullet points (≤ 8 bullets). \
             Include: user goals/requests, key decisions, files created or modified, errors \
             resolved, and any explicit user preferences or constraints. \
             Omit pleasantries and tool call details. \
             This summary is injected at the start of a future context window so the AI \
             retains critical context — be precise and information-dense.",
        ));

        // No tokio::time::timeout — Session is !Sync (RefCell inside rusqlite), so timeout
        // would require Send and fail to compile. HTTP-level timeout is the backstop.
        let resp = self.client.send(
            "You are a concise technical summariser. Extract the most context-critical \
             information from these conversation turns into tight bullet points.",
            &prompt,
            &[],    // no tools
            None,
            0,      // no thinking budget
        ).await?;

        let text = resp.content.iter()
            .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
            .collect::<Vec<_>>()
            .join("\n");

        if text.trim().is_empty() {
            anyhow::bail!("LLM returned empty summary");
        }

        Ok(text)
    }

    /// Zero-cost text fallback used when the LLM summarizer fails.
    /// Extracts first 200 chars of each real user turn + first 250 chars of the
    /// paired assistant reply, plus the names of tools called.
    pub(super) fn text_drop_summary(dropped: &[Message]) -> String {
        const USER_PREVIEW: usize = 200;
        const ASST_PREVIEW: usize = 250;
        const MAX_TOOLS:    usize = 5;

        let real_positions: Vec<usize> = dropped.iter().enumerate()
            .filter(|(_, m)| {
                m.role == "user"
                    && m.content.first()
                        .is_some_and(|b| matches!(b, ContentBlock::Text { .. }))
            })
            .map(|(i, _)| i)
            .collect();

        let mut parts = vec![format!(
            "[Text summary of {} dropped turn{} (LLM summarizer unavailable)]",
            real_positions.len(),
            if real_positions.len() == 1 { "" } else { "s" }
        )];

        for (idx, &pos) in real_positions.iter().enumerate() {
            let next = real_positions.get(idx + 1).copied().unwrap_or(dropped.len());
            let user_text = dropped[pos].content.iter()
                .find_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                .unwrap_or("");
            let user_prev: String = user_text.chars().take(USER_PREVIEW).collect();
            let ellipsis = if user_text.chars().count() > USER_PREVIEW { "…" } else { "" };

            let mut asst_prev = String::new();
            let mut tools: Vec<String> = Vec::new();
            for msg in &dropped[pos + 1..next] {
                for block in &msg.content {
                    match block {
                        ContentBlock::Text { text } if msg.role == "assistant" && asst_prev.is_empty() => {
                            asst_prev = text.chars().take(ASST_PREVIEW).collect();
                            if text.chars().count() > ASST_PREVIEW { asst_prev.push('…'); }
                        }
                        ContentBlock::ToolUse { name, input, .. } if tools.len() < MAX_TOOLS => {
                            let file = input.get("path")
                                .or_else(|| input.get("file_path"))
                                .and_then(|v| v.as_str())
                                .and_then(|p| std::path::Path::new(p).file_name()?.to_str())
                                .map(|f| format!("({})", f))
                                .unwrap_or_default();
                            tools.push(format!("{}{}", name, file));
                        }
                        _ => {}
                    }
                }
            }

            let mut entry = format!("• {}{}", user_prev, ellipsis);
            if !asst_prev.is_empty() { entry.push_str(&format!("\n  → {}", asst_prev)); }
            if !tools.is_empty()     { entry.push_str(&format!("\n  [{}]", tools.join(", "))); }
            parts.push(entry);
        }
        parts.join("\n\n")
    }
}
