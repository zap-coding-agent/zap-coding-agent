use anyhow::Result;
use async_trait::async_trait;
use std::io::Write as _;
use std::process::{Command, Stdio};

use super::{ApiResponse, BeforeOutput, ContentBlock, LlmProvider, Message, Usage};

pub struct ClaudeCodeClient {
    model: String,
    suppress_stream: bool,
}

impl ClaudeCodeClient {
    pub fn new(model: String, suppress_stream: bool) -> Self {
        Self { model, suppress_stream }
    }
}

/// Finds the claude binary from PATH or common brew/system install locations.
fn find_claude() -> &'static str {
    let candidates: &[&str] = &["claude", "/opt/homebrew/bin/claude", "/usr/local/bin/claude"];
    for &c in candidates {
        if Command::new(c).arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
            // Leak is fine — called once, returns a &'static str for the process lifetime.
            return Box::leak(c.to_string().into_boxed_str());
        }
    }
    "claude"
}

/// Encode messages into the NDJSON format `claude -p --input-format stream-json` expects.
/// Tool calls/results are flattened to text so claude receives full conversation context.
fn encode_messages(messages: &[Message]) -> String {
    let mut out = String::new();
    for msg in messages {
        let text_parts: Vec<&str> = msg.content.iter().filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            ContentBlock::ToolResult { content, .. } => Some(content.as_str()),
            _ => None,
        }).collect();

        let text = text_parts.join("\n").trim().to_string();
        if text.is_empty() { continue; }

        let json = serde_json::json!({
            "type": msg.role,
            "message": {
                "role": msg.role,
                "content": [{"type": "text", "text": text}]
            }
        });
        out.push_str(&serde_json::to_string(&json).unwrap_or_default());
        out.push('\n');
    }
    out
}

#[async_trait]
impl LlmProvider for ClaudeCodeClient {
    async fn send(
        &self,
        system: &str,
        messages: &[Message],
        _tools: &[serde_json::Value],
        before_output: Option<BeforeOutput>,
        _thinking_budget: u32,
    ) -> Result<ApiResponse> {
        let mut before_output = before_output;
        let mut highlighter = crate::stream_highlighter::StreamHighlighter::new();
        highlighter.suppress_print = crate::tui::channel::is_tui_mode();

        let stdin_data = encode_messages(messages);
        let claude_bin = find_claude();

        let mut cmd = Command::new(claude_bin);
        cmd.arg("--print")
            .args(["--output-format", "stream-json"])
            .arg("--verbose")
            .args(["--input-format", "stream-json"])
            .args(["--model", &self.model]);

        if !system.is_empty() {
            // Rust's Command::arg avoids shell quoting entirely — long strings are fine.
            cmd.args(["--append-system-prompt", system]);
        }

        cmd.stdin(Stdio::piped())
           .stdout(Stdio::piped())
           .stderr(Stdio::null());

        let mut child = cmd.spawn()
            .map_err(|e| anyhow::anyhow!("Failed to launch claude CLI: {e}. Is Claude Code installed?"))?;

        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(stdin_data.as_bytes());
            // Drop closes stdin, signalling EOF to claude.
        }

        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("No stdout from claude process"))?;

        // Read subprocess stdout in a blocking thread and forward lines via channel.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        tokio::task::spawn_blocking(move || {
            use std::io::BufRead as _;
            for l in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                if !l.is_empty() { let _ = tx.send(l); }
            }
        });

        let mut full_text = String::new();
        let mut prev_len = 0usize;
        let mut stop_reason = "end_turn".to_string();
        let mut usage = Usage::default();

        while let Some(line) = rx.recv().await {
            let Ok(ev) = serde_json::from_str::<serde_json::Value>(&line) else { continue };

            match ev["type"].as_str().unwrap_or("") {
                "assistant" => {
                    // Collect text from all content blocks in this event.
                    let text: String = ev["message"]["content"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|b| {
                                    if b["type"].as_str() == Some("text") {
                                        b["text"].as_str().map(|s| s.to_string())
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>()
                                .join("")
                        })
                        .unwrap_or_default();

                    // Emit only the new portion since last event.
                    if text.len() > prev_len {
                        let chunk = &text[prev_len..];
                        crate::remote_channel::send_chunk(chunk);
                        if !self.suppress_stream {
                            if let Some(cb) = before_output.take() { cb(); }
                            highlighter.push(chunk);
                        }
                        prev_len = text.len();
                    }
                    full_text = text;

                    if let Some(u) = ev["message"]["usage"].as_object() {
                        usage.input_tokens  = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        usage.output_tokens += u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        usage.cache_read_tokens  = u.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        usage.cache_write_tokens = u.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    }
                }

                "result" => {
                    if let Some(r) = ev["stop_reason"].as_str() { stop_reason = r.to_string(); }
                    // Final usage (authoritative totals from claude).
                    if let Some(u) = ev["usage"].as_object() {
                        if let Some(v) = u.get("input_tokens").and_then(|v| v.as_u64())  { usage.input_tokens  = v as u32; }
                        if let Some(v) = u.get("output_tokens").and_then(|v| v.as_u64()) { usage.output_tokens = v as u32; }
                        if let Some(v) = u.get("cache_read_input_tokens").and_then(|v| v.as_u64()) { usage.cache_read_tokens = v as u32; }
                    }
                    // Fallback: if no assistant events came through, use the result text.
                    if full_text.is_empty() {
                        if let Some(t) = ev["result"].as_str() {
                            full_text = t.to_string();
                            crate::remote_channel::send_chunk(&full_text);
                            if !self.suppress_stream {
                                if let Some(cb) = before_output.take() { cb(); }
                                highlighter.push(&full_text);
                            }
                        }
                    }
                }

                "system" if ev["subtype"].as_str() == Some("error") => {
                    let msg = ev["error"]["message"].as_str().unwrap_or("unknown error");
                    anyhow::bail!("Claude Code error: {msg}");
                }

                _ => {}
            }
        }

        let _ = child.wait();

        if full_text.is_empty() {
            anyhow::bail!(
                "Claude Code returned empty response. \
                 Ensure `claude` CLI is installed and authenticated (run `claude` once to log in)."
            );
        }

        Ok(ApiResponse {
            content: vec![ContentBlock::Text { text: full_text }],
            stop_reason,
            usage: Some(usage),
        })
    }
}
