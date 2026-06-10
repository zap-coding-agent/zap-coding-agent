use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use serde::Serialize;

use super::{
    ApiResponse, BeforeOutput, ContentBlock, Message, Usage,
    build_curl_block, check_tool_support_error, normalize_anthropic_url, redact_token,
    send_with_retry, LlmProvider,
};

const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Documented output token caps for known Anthropic model families.
/// Uses a conservative default for unrecognized models.
fn max_output_tokens(model: &str) -> u32 {
    let m = model.to_lowercase();
    if m.contains("claude-opus") || m.contains("claude-sonnet") { 32_000 }
    else                                                          { 16_000 }
}

#[derive(Debug, Serialize)]
pub(super) struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: Vec<serde_json::Value>,
    messages: Vec<serde_json::Value>,
    tools: Vec<serde_json::Value>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<serde_json::Value>,
}

/// Encode our internal messages into the Anthropic wire format.
/// Handles the nested `source` object required for image blocks.
fn encode_messages_anthropic(messages: &[Message]) -> Vec<serde_json::Value> {
    messages.iter().map(|msg| {
        let content: Vec<serde_json::Value> = msg.content.iter().map(|block| {
            match block {
                ContentBlock::Text { text } =>
                    serde_json::json!({ "type": "text", "text": text }),

                ContentBlock::ToolUse { id, name, input } =>
                    serde_json::json!({ "type": "tool_use", "id": id, "name": name, "input": input }),

                ContentBlock::ToolResult { tool_use_id, content } =>
                    serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                        "is_error": false,
                    }),

                ContentBlock::Image { media_type, data } =>
                    serde_json::json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": media_type, "data": data }
                    }),

                ContentBlock::Thinking { thinking, signature } =>
                    serde_json::json!({
                        "type": "thinking",
                        "thinking": thinking,
                        "signature": signature,
                    }),

                // Reasoning blocks are DeepSeek-specific — not sent to Anthropic.
                ContentBlock::Reasoning { .. } =>
                    serde_json::json!({ "type": "text", "text": "" }),
            }
        }).collect();

        serde_json::json!({ "role": msg.role, "content": content })
    }).collect()
}

/// Mark the last content block of the last message with `cache_control: ephemeral`
/// so Anthropic can serve the conversation prefix from cache across turns.
/// Prefers the last text block; falls back to the last block of any type.
fn mark_last_message_cacheable(messages: &mut [serde_json::Value]) {
    let Some(last_msg) = messages.last_mut() else { return };
    let content = match last_msg["content"].as_array_mut() {
        Some(c) if !c.is_empty() => c,
        _ => return,
    };

    // Prefer the last text block, otherwise the last block of any type.
    let target_idx = content
        .iter()
        .enumerate()
        .rev()
        .find(|(_, b)| b["type"].as_str() == Some("text"))
        .map(|(i, _)| i)
        .or_else(|| if content.is_empty() { None } else { Some(content.len() - 1) });

    if let Some(idx) = target_idx {
        if let Some(obj) = content[idx].as_object_mut() {
            obj.insert(
                "cache_control".to_string(),
                serde_json::json!({ "type": "ephemeral" }),
            );
        }
    }
}

/// Per-content-block accumulator while parsing the SSE stream.
#[derive(Default)]
struct BlockAccum {
    kind: String,       // "text", "tool_use", or "thinking"
    id: String,
    name: String,
    text: String,       // accumulated text / thinking content
    signature: String,  // thinking block signature (opaque, required for multi-turn)
    input_json: String, // accumulated tool input JSON
}

pub(super) struct AnthropicClient {
    http: reqwest::Client,
    credential: super::CredentialProvider,
    model: String,
    url: String,
    suppress_stream: bool,
    bearer_auth: bool,
    disable_stream: bool,
}

impl AnthropicClient {
    pub(super) fn new(
        credential: super::CredentialProvider,
        model: String,
        base_url: Option<String>,
        suppress_stream: bool,
        disable_stream: bool,
    ) -> Self {
        let bearer_auth = base_url.is_some();
        let url = normalize_anthropic_url(base_url.as_deref());
        Self {
            http: crate::http::client().clone(),
            credential,
            model,
            url,
            suppress_stream,
            bearer_auth,
            disable_stream,
        }
    }

    fn process_event(
        event: &serde_json::Value,
        blocks: &mut std::collections::HashMap<usize, BlockAccum>,
        stop_reason: &mut String,
        before_output: &mut Option<BeforeOutput>,
        usage: &mut Usage,
        suppress_stream: bool,
        highlighter: &mut crate::stream_highlighter::StreamHighlighter,
    ) {
        let t = event["type"].as_str().unwrap_or("");
        match t {
            "message_start" => {
                if let Some(u) = event["message"]["usage"].as_object() {
                    usage.input_tokens = u.get("input_tokens")
                        .and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    usage.cache_read_tokens = u.get("cache_read_input_tokens")
                        .and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                    usage.cache_write_tokens = u.get("cache_creation_input_tokens")
                        .and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                }
            }
            "content_block_start" => {
                let idx = event["index"].as_u64().unwrap_or(0) as usize;
                let cb = &event["content_block"];
                let kind = cb["type"].as_str().unwrap_or("").to_string();
                let mut acc = BlockAccum { kind: kind.clone(), ..Default::default() };
                if kind == "tool_use" {
                    acc.id   = cb["id"].as_str().unwrap_or("").to_string();
                    acc.name = cb["name"].as_str().unwrap_or("").to_string();
                }
                blocks.insert(idx, acc);
            }
            "content_block_delta" => {
                let idx = event["index"].as_u64().unwrap_or(0) as usize;
                let delta = &event["delta"];
                let dtype = delta["type"].as_str().unwrap_or("");
                if let Some(acc) = blocks.get_mut(&idx) {
                    if dtype == "text_delta" {
                        let chunk = delta["text"].as_str().unwrap_or("");
                        if !chunk.is_empty() {
                            crate::remote_channel::send_chunk(chunk);
                            if !suppress_stream {
                                if let Some(cb) = before_output.take() { cb(); }
                                highlighter.push(chunk);
                                let _ = std::io::Write::flush(&mut std::io::stdout());
                            }
                        }
                        acc.text.push_str(chunk);
                    } else if dtype == "thinking_delta" {
                        let chunk = delta["thinking"].as_str().unwrap_or("");
                        if !chunk.is_empty() {
                            if crate::tui::channel::is_tui_mode() {
                                if let Some(cb) = before_output.take() { cb(); }
                                crate::tui::channel::tui_send(
                                    crate::tui::channel::TuiEvent::ThinkingChunk(chunk.to_string())
                                );
                            }
                            acc.text.push_str(chunk);
                        }
                    } else if dtype == "input_json_delta" {
                        acc.input_json
                            .push_str(delta["partial_json"].as_str().unwrap_or(""));
                    } else if dtype == "signature_delta" {
                        acc.signature.push_str(delta["signature"].as_str().unwrap_or(""));
                    }
                }
            }
            "message_delta" => {
                if let Some(reason) = event["delta"]["stop_reason"].as_str() {
                    *stop_reason = reason.to_string();
                }
                if let Some(u) = event["usage"].as_object() {
                    usage.output_tokens = u.get("output_tokens")
                        .and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                }
            }
            _ => {}
        }
    }
}

#[async_trait]
impl LlmProvider for AnthropicClient {
    async fn send(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[serde_json::Value],
        before_output: Option<BeforeOutput>,
        thinking_budget: u32,
    ) -> Result<ApiResponse> {
        let mut before_output = before_output;
        let mut highlighter = crate::stream_highlighter::StreamHighlighter::new();
        highlighter.suppress_print = crate::tui::channel::is_tui_mode();
        let api_key = self.credential.get().map_err(|e| anyhow::anyhow!("{e}"))?;
        if api_key.is_empty() {
            anyhow::bail!("ANTHROPIC_API_KEY is not set");
        }

        let system_blocks = vec![serde_json::json!({
            "type": "text",
            "text": system,
            "cache_control": { "type": "ephemeral" }
        })];

        let mut cached_tools: Vec<serde_json::Value> = tools.to_vec();
        if let Some(last) = cached_tools.last_mut() {
            if let Some(obj) = last.as_object_mut() {
                obj.insert("cache_control".to_string(),
                    serde_json::json!({ "type": "ephemeral" }));
            }
        }

        let effective_budget = thinking_budget
            .min(max_output_tokens(&self.model).saturating_sub(1));
        let thinking = if effective_budget > 0 {
            Some(serde_json::json!({"type": "enabled", "budget_tokens": effective_budget}))
        } else {
            None
        };
        let mut encoded = encode_messages_anthropic(messages);
        mark_last_message_cacheable(&mut encoded);
        let body = AnthropicRequest {
            model: &self.model,
            max_tokens: max_output_tokens(&self.model),
            system: system_blocks,
            messages: encoded,
            tools: cached_tools,
            stream: !self.disable_stream,
            thinking,
        };
        let body_bytes = serde_json::to_vec(&body).context("failed to serialize request")?;

        if self.bearer_auth {
            for msg in messages {
                for block in &msg.content {
                    if let ContentBlock::ToolResult { tool_use_id, content } = block {
                        if content.is_empty() {
                            crate::zap_warn!(
                                "Tool result for '{}' has empty content before being sent to the gateway. \
                                 If the model reports empty tool results, your corporate proxy may be \
                                 stripping tool result content (DLP policy). Check ~/.zap/llm.log to \
                                 see exactly what was sent.",
                                tool_use_id
                            );
                        }
                    }
                }
            }
        }

        if let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
            let n = v["tools"].as_array().map(|t| t.len()).unwrap_or(0);

            let auth_val = if self.bearer_auth {
                format!("Bearer {}", api_key)
            } else {
                api_key.clone()
            };
            let auth_hdr = if self.bearer_auth { "Authorization" } else { "x-api-key" };
            let curl = build_curl_block("anthropic", &self.url, auth_hdr, &auth_val, &v);

            v["tools"] = serde_json::json!(format!("<{n} tools — omitted>"));
            if let Ok(pretty) = serde_json::to_string_pretty(&v) {
                let auth_line = format!("{}: {}", auth_hdr, redact_token(&auth_val));
                crate::log::write_llm(
                    "REQUEST [anthropic]",
                    &format!("POST {}\n{}\n\n{}{}", self.url, auth_line, pretty, curl),
                );
            }
        }

        let bearer_auth = self.bearer_auth;
        let resp = send_with_retry(&self.http, |http| {
            let mut req = http.post(&self.url)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("content-type", "application/json")
                .body(body_bytes.clone());
            if bearer_auth {
                req = req.header("Authorization", format!("Bearer {}", api_key));
            } else {
                req = req.header("x-api-key", &api_key);
            }
            req
        })
        .await
        .context("failed to reach Anthropic API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            crate::log::write_llm("ERROR [anthropic]", &format!("HTTP {} — {}", status, text));
            check_tool_support_error(status.as_u16(), &text);
            anyhow::bail!("Anthropic API returned {} (url: {}): {}", status, self.url, text);
        }

        let (content, stop_reason, usage_acc) = if self.disable_stream {
            let text = resp.text().await.context("failed to read Anthropic response")?;
            let json: serde_json::Value = serde_json::from_str(&text)
                .context("failed to parse Anthropic JSON response")?;

            let stop_reason = json["stop_reason"].as_str().unwrap_or("end_turn").to_string();
            let usage_acc = if let Some(u) = json["usage"].as_object() {
                Usage {
                    input_tokens:       u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    output_tokens:      u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    cache_read_tokens:  u.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    cache_write_tokens: u.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                }
            } else {
                Usage::default()
            };

            let mut content: Vec<ContentBlock> = Vec::new();
            if let Some(blocks) = json["content"].as_array() {
                for block in blocks {
                    match block["type"].as_str().unwrap_or("") {
                        "text" => {
                            let t = block["text"].as_str().unwrap_or("").to_string();
                            if !t.is_empty() {
                                if !self.suppress_stream {
                                    if let Some(cb) = before_output.take() { cb(); }
                                    highlighter.push(&t);
                                    highlighter.flush();
                                }
                                content.push(ContentBlock::Text { text: t });
                            }
                        }
                        "tool_use" => {
                            let id    = block["id"].as_str().unwrap_or("").to_string();
                            let name  = block["name"].as_str().unwrap_or("").to_string();
                            let input = block["input"].clone();
                            content.push(ContentBlock::ToolUse { id, name, input });
                        }
                        "thinking" => {
                            let thinking  = block["thinking"].as_str().unwrap_or("").to_string();
                            let signature = block["signature"].as_str().unwrap_or("").to_string();
                            if !thinking.is_empty() {
                                content.push(ContentBlock::Thinking { thinking, signature });
                            }
                        }
                        _ => {}
                    }
                }
            }
            if let Some(cb) = before_output.take() { cb(); }
            (content, stop_reason, usage_acc)
        } else {
            let mut stream = resp.bytes_stream();
            let mut buf = String::new();
            let mut blocks: std::collections::HashMap<usize, BlockAccum> = Default::default();
            let mut stop_reason = "end_turn".to_string();
            let mut usage_acc = Usage::default();

            while let Some(chunk) = stream.next().await {
                let bytes: bytes::Bytes = chunk.context("SSE stream error")?;
                buf.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim_end_matches('\r').to_string();
                    buf = buf[pos + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" { break; }
                        if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                            Self::process_event(
                                &event,
                                &mut blocks,
                                &mut stop_reason,
                                &mut before_output,
                                &mut usage_acc,
                                self.suppress_stream,
                                &mut highlighter,
                            );
                        }
                    }
                }
            }

            let mut pairs: Vec<(usize, BlockAccum)> = blocks.into_iter().collect();
            pairs.sort_by_key(|(idx, _)| *idx);

            let had_text = pairs.iter().any(|(_, a)| a.kind == "text" && !a.text.is_empty());
            if had_text && !self.suppress_stream {
                highlighter.flush();
            }

            let mut content: Vec<ContentBlock> = Vec::new();
            for (_, acc) in pairs {
                match acc.kind.as_str() {
                    "text" if !acc.text.is_empty() =>
                        content.push(ContentBlock::Text { text: acc.text }),
                    "tool_use" => {
                        let input: serde_json::Value =
                            serde_json::from_str(&acc.input_json).unwrap_or(serde_json::json!({}));
                        content.push(ContentBlock::ToolUse { id: acc.id, name: acc.name, input });
                    }
                    "thinking" if !acc.text.is_empty() =>
                        content.push(ContentBlock::Thinking {
                            thinking: acc.text,
                            signature: acc.signature,
                        }),
                    _ => {}
                }
            }

            if let Some(cb) = before_output.take() { cb(); }
            (content, stop_reason, usage_acc)
        };

        {
            let resp_val = serde_json::json!({
                "stop_reason": stop_reason,
                "usage": {
                    "input_tokens":       usage_acc.input_tokens,
                    "output_tokens":      usage_acc.output_tokens,
                    "cache_read_tokens":  usage_acc.cache_read_tokens,
                    "cache_write_tokens": usage_acc.cache_write_tokens,
                },
                "content": content.iter().map(|b| match b {
                    ContentBlock::Text { text } =>
                        serde_json::json!({ "type": "text", "text": text }),
                    ContentBlock::ToolUse { id, name, input } =>
                        serde_json::json!({ "type": "tool_use", "id": id, "name": name, "input": input }),
                    ContentBlock::ToolResult { tool_use_id, content } =>
                        serde_json::json!({ "type": "tool_result", "tool_use_id": tool_use_id, "content": content }),
                    ContentBlock::Image { .. } =>
                        serde_json::json!({ "type": "image", "data": "<redacted>" }),
                    ContentBlock::Thinking { thinking, .. } =>
                        serde_json::json!({ "type": "thinking", "thinking": format!("<{} chars>", thinking.len()) }),
                    ContentBlock::Reasoning { content } =>
                        serde_json::json!({ "type": "reasoning", "reasoning": format!("<{} chars>", content.len()) }),
                }).collect::<Vec<_>>(),
            });
            if let Ok(pretty) = serde_json::to_string_pretty(&resp_val) {
                crate::log::write_llm("RESPONSE [anthropic]", &pretty);
            }
        }

        if self.bearer_auth {
            let had_tool_results = messages.iter().any(|m| {
                m.content.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }))
            });
            if had_tool_results {
                let response_text: String = content.iter().filter_map(|b| {
                    if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
                }).collect::<Vec<_>>().join(" ").to_lowercase();
                let dlp_patterns = ["empty result", "no result", "returned empty", "empty response",
                                    "no content", "empty output", "no output", "empty string",
                                    "nothing was returned", "did not return"];
                if dlp_patterns.iter().any(|p| response_text.contains(p)) {
                    crate::zap_warn!(
                        "The model reported empty tool results while using a custom gateway. \
                         Your corporate proxy may be stripping tool_result content via DLP policy. \
                         Check ~/.zap/llm.log to verify what was sent — if tool results appear there \
                         but the model still sees empty content, contact your IT team about the \
                         gateway's content inspection rules for API requests."
                    );
                }
            }
        }

        Ok(ApiResponse { content, stop_reason, usage: Some(usage_acc) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_last_message_adds_cache_control() {
        let messages = vec![
            Message {
                role: "user".into(),
                content: vec![ContentBlock::Text { text: "hello".into() }],
            },
            Message {
                role: "assistant".into(),
                content: vec![ContentBlock::Text { text: "hi there".into() }],
            },
        ];
        let mut encoded = encode_messages_anthropic(&messages);
        mark_last_message_cacheable(&mut encoded);

        // First message should NOT have cache_control
        let first_blocks = encoded[0]["content"].as_array().unwrap();
        for block in first_blocks {
            assert!(block.get("cache_control").is_none(),
                "first message should not have cache_control");
        }

        // Last message's last block SHOULD have cache_control
        let last_blocks = encoded[1]["content"].as_array().unwrap();
        let last_block = last_blocks.last().unwrap();
        let cc = last_block["cache_control"].as_object().unwrap();
        assert_eq!(cc["type"].as_str().unwrap(), "ephemeral");
    }

    #[test]
    fn mark_last_message_skips_if_empty() {
        let mut encoded: Vec<serde_json::Value> = vec![];
        mark_last_message_cacheable(&mut encoded);
        // Should not panic
        assert!(encoded.is_empty());
    }
}
