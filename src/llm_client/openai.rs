use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;

use super::{
    ApiResponse, BeforeOutput, ContentBlock, Message, Usage,
    build_curl_block, check_text_mode_tool_call, normalize_openai_url, redact_token,
    send_with_retry, LlmProvider,
};

/// Per-tool-call accumulator for the OpenAI SSE stream.
#[derive(Default)]
struct OaiToolAccum {
    id: String,
    name: String,
    arguments: String,
}

pub(super) struct OpenAiClient {
    http: reqwest::Client,
    pub(super) credential: super::CredentialProvider,
    model: String,
    url: String,
    suppress_stream: bool,
    disable_stream: bool,
    image_support: bool,
    /// Auth header name — "Authorization" (default) or "x-goog-api-key" (Gemini API keys).
    pub(super) auth_header: String,
}

impl OpenAiClient {
    pub(super) fn new(
        credential: super::CredentialProvider,
        model: String,
        base_url: Option<String>,
        suppress_stream: bool,
        disable_stream: bool,
        auth_header: Option<String>,
    ) -> Self {
        let url = normalize_openai_url(base_url.as_deref());
        let image_support = !url.contains("deepseek.com");
        let auth_header = auth_header.unwrap_or_else(|| "Authorization".to_string());
        Self {
            http: crate::http::client().clone(),
            credential,
            model,
            url,
            suppress_stream,
            disable_stream,
            image_support,
            auth_header,
        }
    }

    fn encode_messages(&self, system: &str, messages: &[Message]) -> Vec<serde_json::Value> {
        let mut out = vec![serde_json::json!({ "role": "system", "content": system })];

        let last_user_idx = messages.iter().rposition(|m| m.role == "user");

        for (idx, msg) in messages.iter().enumerate() {
            match msg.role.as_str() {
                "user" => {
                    let tool_results: Vec<&ContentBlock> = msg
                        .content
                        .iter()
                        .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
                        .collect();

                    if tool_results.is_empty() {
                        let parts: Vec<serde_json::Value> = msg.content.iter().filter_map(|b| {
                            match b {
                                ContentBlock::Text { text } =>
                                    Some(serde_json::json!({ "type": "text", "text": text })),
                                ContentBlock::Image { media_type, data } => {
                                    if self.image_support {
                                        Some(serde_json::json!({ "type": "image_url", "image_url": { "url": format!("data:{};base64,{}", media_type, data) } }))
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            }
                        }).collect();

                        if !self.image_support
                            && parts.len() < msg.content.len()
                            && Some(idx) == last_user_idx
                        {
                            let dropped = msg.content.iter()
                                .filter(|b| matches!(b, ContentBlock::Image { .. }))
                                .count();
                            crate::zap_warn!("Dropping {dropped} image block(s): the model at '{}' does not support vision.", self.url);
                        }

                        let content = if parts.len() == 1 {
                            if let serde_json::Value::String(_) = &parts[0] {
                                parts[0].clone()
                            } else if let Some(t) = parts[0].get("text").and_then(|v| v.as_str()) {
                                serde_json::Value::String(t.to_string())
                            } else {
                                serde_json::Value::Array(parts)
                            }
                        } else {
                            serde_json::Value::Array(parts)
                        };

                        out.push(serde_json::json!({ "role": "user", "content": content }));
                    } else {
                        for block in tool_results {
                            if let ContentBlock::ToolResult { tool_use_id, content } = block {
                                out.push(serde_json::json!({
                                    "role": "tool",
                                    "tool_call_id": tool_use_id,
                                    "content": content,
                                }));
                            }
                        }
                    }
                }
                "assistant" => {
                    let reasoning = msg.content.iter().find_map(|b| {
                        if let ContentBlock::Reasoning { content } = b { Some(content.clone()) } else { None }
                    });
                    let text = msg.content.iter().find_map(|b| {
                        if let ContentBlock::Text { text } = b { Some(text.clone()) } else { None }
                    });

                    let tool_calls: Vec<serde_json::Value> = msg
                        .content
                        .iter()
                        .filter_map(|b| {
                            if let ContentBlock::ToolUse { id, name, input } = b {
                                Some(serde_json::json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": serde_json::to_string(input)
                                            .unwrap_or_default(),
                                    }
                                }))
                            } else {
                                None
                            }
                        })
                        .collect();

                    let mut m = serde_json::json!({ "role": "assistant" });
                    m["content"] = text.map(serde_json::Value::String)
                        .unwrap_or(serde_json::Value::Null);
                    if !tool_calls.is_empty() {
                        m["tool_calls"] = serde_json::json!(tool_calls);
                    }
                    if let Some(rc) = reasoning {
                        m["reasoning_content"] = serde_json::Value::String(rc);
                    }
                    out.push(m);
                }
                _ => {}
            }
        }
        out
    }

    fn encode_tools(tools: &[serde_json::Value]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|t| serde_json::json!({
                "type": "function",
                "function": {
                    "name": t["name"],
                    "description": t["description"],
                    "parameters": t["input_schema"],
                }
            }))
            .collect()
    }
}

#[async_trait]
impl LlmProvider for OpenAiClient {
    async fn send(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[serde_json::Value],
        before_output: Option<BeforeOutput>,
        _thinking_budget: u32,
    ) -> Result<ApiResponse> {
        let mut before_output = before_output;
        let mut highlighter = crate::stream_highlighter::StreamHighlighter::new();
        highlighter.suppress_print = crate::tui::channel::is_tui_mode();
        let oai_messages = self.encode_messages(system, messages);
        let oai_tools = Self::encode_tools(tools);

        let mut body = if self.disable_stream {
            serde_json::json!({
                "model": self.model,
                "messages": oai_messages,
                "stream": false,
            })
        } else {
            serde_json::json!({
                "model": self.model,
                "messages": oai_messages,
                "stream": true,
                "stream_options": { "include_usage": true },
            })
        };
        if !oai_tools.is_empty() {
            body["tools"] = serde_json::json!(oai_tools);
        }
        let body_bytes = serde_json::to_vec(&body).context("failed to serialize request")?;

        let tools_were_sent = !oai_tools.is_empty();

        // Fetch the credential (static key or gcloud ADC token).
        let api_key = self.credential.get().map_err(|e| anyhow::anyhow!("{e}"))?;

        if let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
            let n = v["tools"].as_array().map(|t| t.len()).unwrap_or(0);

            let auth_val = if api_key.is_empty() {
                String::new()
            } else if self.auth_header == "Authorization" {
                format!("Bearer {}", api_key)
            } else {
                // Custom header like x-goog-api-key — value is the key directly.
                api_key.clone()
            };
            let curl = if auth_val.is_empty() {
                build_curl_block("openai", &self.url, &self.auth_header, "", &v)
            } else {
                build_curl_block("openai", &self.url, &self.auth_header, &auth_val, &v)
            };

            if n > 0 {
                v["tools"] = serde_json::json!(format!("<{n} tools — omitted>"));
            }
            if let Ok(pretty) = serde_json::to_string_pretty(&v) {
                let auth_line = if auth_val.is_empty() {
                    "(no auth)".to_string()
                } else {
                    format!("{}: {}", self.auth_header, redact_token(&auth_val))
                };
                crate::log::write_llm(
                    "REQUEST [openai]",
                    &format!("POST {}\n{}\n\n{}{}", self.url, auth_line, pretty, curl),
                );
            }
        }

        let auth_header = &self.auth_header;
        let resp = send_with_retry(&self.http, |http| {
            let mut req = http
                .post(&self.url)
                .header("content-type", "application/json")
                .body(body_bytes.clone());
            // GcloudAdc always sends Authorization header (even empty) — Gemini accepts
            // "Bearer " (empty token) but rejects missing auth entirely (returns 400).
            let send_header = !api_key.is_empty() || self.credential.always_send_auth_header();
            if send_header {
                let value = if auth_header == "Authorization" {
                    format!("Bearer {}", api_key)
                } else {
                    api_key.clone()
                };
                req = req.header(auth_header.as_str(), &value);
            }
            req
        })
        .await
        .context("failed to reach OpenAI-compatible API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            crate::log::write_llm("ERROR [openai]", &format!("HTTP {} — {}", status, text));
            check_text_mode_tool_call(&text, tools_were_sent);
            anyhow::bail!("OpenAI API returned {} (url: {}): {}", status, self.url, text);
        }

        let (content, stop_reason, usage) = if self.disable_stream {
            let text = resp.text().await.context("failed to read OpenAI response")?;
            let json: serde_json::Value = serde_json::from_str(&text)
                .context("failed to parse OpenAI JSON response")?;

            let choice = &json["choices"][0];
            let message = &choice["message"];
            let finish_reason = choice["finish_reason"].as_str().unwrap_or("stop").to_string();

            let mut content: Vec<ContentBlock> = Vec::new();
            if let Some(rc) = message["reasoning_content"].as_str() {
                if !rc.is_empty() {
                    content.push(ContentBlock::Reasoning { content: rc.to_string() });
                }
            }
            if let Some(t) = message["content"].as_str() {
                if !t.is_empty() {
                    if !self.suppress_stream {
                        if let Some(cb) = before_output.take() { cb(); }
                        highlighter.push(t);
                        highlighter.flush();
                    }
                    content.push(ContentBlock::Text { text: t.to_string() });
                }
            }
            if let Some(tool_calls) = message["tool_calls"].as_array() {
                for tc in tool_calls {
                    let id    = tc["id"].as_str().unwrap_or("").to_string();
                    let name  = tc["function"]["name"].as_str().unwrap_or("").to_string();
                    let args  = tc["function"]["arguments"].as_str().unwrap_or("{}");
                    let input: serde_json::Value =
                        serde_json::from_str(args).unwrap_or(serde_json::json!({}));
                    content.push(ContentBlock::ToolUse { id, name, input });
                }
            }

            if message["tool_calls"].is_null() && finish_reason == "stop" {
                if let Some(t) = message["content"].as_str() {
                    check_text_mode_tool_call(t, tools_were_sent);
                }
            }

            let usage_acc = if let Some(u) = json["usage"].as_object() {
                Usage {
                    input_tokens:  u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    output_tokens: u.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                    ..Usage::default()
                }
            } else {
                Usage::default()
            };

            if let Some(cb) = before_output.take() { cb(); }

            let stop_reason = match finish_reason.as_str() {
                "tool_calls" => "tool_use".to_string(),
                _ => "end_turn".to_string(),
            };
            let usage = if usage_acc.input_tokens > 0 || usage_acc.output_tokens > 0 {
                Some(usage_acc)
            } else {
                None
            };
            (content, stop_reason, usage)
        } else {
            let mut stream = resp.bytes_stream();
            let mut buf = String::new();
            let mut text_acc = String::new();
            let mut reasoning_acc = String::new();
            let mut tool_accums: std::collections::HashMap<usize, OaiToolAccum> = Default::default();
            let mut finish_reason = "stop".to_string();
            let mut usage_acc = Usage::default();

            'outer: while let Some(chunk) = stream.next().await {
                let bytes: bytes::Bytes = chunk.context("SSE stream error")?;
                buf.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim_end_matches('\r').to_string();
                    buf = buf[pos + 1..].to_string();

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" { break 'outer; }
                        if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                            let choice = &event["choices"][0];
                            let delta = &choice["delta"];

                            if let Some(rc) = delta["reasoning_content"].as_str() {
                                if !rc.is_empty() { reasoning_acc.push_str(rc); }
                            }

                            if let Some(text) = delta["content"].as_str() {
                                if !text.is_empty() {
                                    crate::remote_channel::send_chunk(text);
                                    if !self.suppress_stream {
                                        if let Some(cb) = before_output.take() { cb(); }
                                        highlighter.push(text);
                                        let _ = std::io::Write::flush(&mut std::io::stdout());
                                    }
                                    text_acc.push_str(text);
                                }
                            }

                            if let Some(tc_arr) = delta["tool_calls"].as_array() {
                                for tc in tc_arr {
                                    let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                                    let acc = tool_accums.entry(idx).or_default();
                                    if let Some(id) = tc["id"].as_str() { acc.id = id.to_string(); }
                                    if let Some(name) = tc["function"]["name"].as_str() { acc.name = name.to_string(); }
                                    if let Some(args) = tc["function"]["arguments"].as_str() { acc.arguments.push_str(args); }
                                }
                            }

                            if let Some(fr) = choice["finish_reason"].as_str() {
                                if !fr.is_empty() { finish_reason = fr.to_string(); }
                            }

                            if let Some(u) = event["usage"].as_object() {
                                if let Some(v) = u.get("prompt_tokens").and_then(|v| v.as_u64()) {
                                    usage_acc.input_tokens = v as u32;
                                }
                                if let Some(v) = u.get("completion_tokens").and_then(|v| v.as_u64()) {
                                    usage_acc.output_tokens = v as u32;
                                }
                            }
                        }
                    }
                }
            }

            if !text_acc.is_empty() && !self.suppress_stream {
                highlighter.flush();
            }
            if let Some(cb) = before_output.take() { cb(); }

            if tool_accums.is_empty() && finish_reason == "stop" {
                check_text_mode_tool_call(&text_acc, tools_were_sent);
            }

            let mut content: Vec<ContentBlock> = Vec::new();
            if !reasoning_acc.is_empty() {
                content.push(ContentBlock::Reasoning { content: reasoning_acc });
            }
            if !text_acc.is_empty() {
                content.push(ContentBlock::Text { text: text_acc });
            }
            let mut pairs: Vec<(usize, OaiToolAccum)> = tool_accums.into_iter().collect();
            pairs.sort_by_key(|(idx, _)| *idx);
            for (_, acc) in pairs {
                let input: serde_json::Value =
                    serde_json::from_str(&acc.arguments).unwrap_or(serde_json::json!({}));
                content.push(ContentBlock::ToolUse { id: acc.id, name: acc.name, input });
            }

            let stop_reason = match finish_reason.as_str() {
                "tool_calls" => "tool_use".to_string(),
                _ => "end_turn".to_string(),
            };
            let usage = if usage_acc.input_tokens > 0 || usage_acc.output_tokens > 0 {
                Some(usage_acc)
            } else {
                None
            };
            (content, stop_reason, usage)
        };

        {
            let resp_val = serde_json::json!({
                "stop_reason": stop_reason,
                "usage": {
                    "input_tokens":  usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
                    "output_tokens": usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
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
                crate::log::write_llm("RESPONSE [openai]", &pretty);
            }
        }

        Ok(ApiResponse { content, stop_reason, usage })
    }
}
