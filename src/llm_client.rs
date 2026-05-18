use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use crate::config::{Config, OutputFormat, Provider};

const MAX_TOKENS: u32 = 16_000;
const MAX_RETRIES: u32 = 5;

// ── Shared types (internal representation) ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String },
    /// Base64-encoded image (jpeg, png, gif, webp). Sent via /attach.
    Image { media_type: String, data: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }

    pub fn tool_results(results: Vec<ContentBlock>) -> Self {
        Self { role: "user".to_string(), content: results }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
}

#[derive(Debug)]
pub struct ApiResponse {
    pub content: Vec<ContentBlock>,
    pub stop_reason: String,
    pub usage: Option<Usage>,
}

// ── Provider trait ────────────────────────────────────────────────────────────

/// Called once, synchronously, immediately before the first output character
/// is written to stdout. Use this to clear a spinner before streaming begins.
pub type BeforeOutput = Box<dyn FnOnce() + Send>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn send(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[serde_json::Value],
        before_output: Option<BeforeOutput>,
    ) -> Result<ApiResponse>;
}

// ── Factory ───────────────────────────────────────────────────────────────────

pub fn create_client(config: &Config) -> Box<dyn LlmProvider> {
    let suppress = config.output_format == OutputFormat::Json;
    match config.provider {
        Provider::Anthropic => Box::new(AnthropicClient::new(
            config.api_key.clone(),
            config.model.clone(),
            config.base_url.clone(),
            suppress,
        )),
        Provider::OpenAi => Box::new(OpenAiClient::new(
            config.api_key.clone(),
            config.model.clone(),
            config.base_url.clone(),
            suppress,
        )),
    }
}

// ── Retry helper ──────────────────────────────────────────────────────────────

/// Send `body_bytes` with `send_fn`, retrying up to MAX_RETRIES times on 429.
async fn send_with_retry(
    http: &reqwest::Client,
    build: impl Fn(&reqwest::Client) -> reqwest::RequestBuilder,
) -> Result<reqwest::Response> {
    let mut last_resp = None;
    for attempt in 0..MAX_RETRIES {
        let resp = build(http).send().await?;
        if resp.status().as_u16() != 429 {
            return Ok(resp);
        }
        // Honour Retry-After if present, else exponential back-off.
        let delay_ms: u64 = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|s| s * 1_000)
            .unwrap_or(5_000 << attempt); // 5 s, 10 s, 20 s, 40 s, 80 s
        let remaining = MAX_RETRIES - attempt - 1;
        if remaining > 0 {
            println!("  ⚠ rate limited — retrying in {}s… ({} attempt(s) left)",
                delay_ms / 1_000, remaining);
            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
        } else {
            last_resp = Some(resp);
        }
    }
    // All retries exhausted — return the last 429 response so the caller
    // can surface a clean error with the response body.
    Ok(last_resp.unwrap())
}

// ── Anthropic streaming client ────────────────────────────────────────────────

const ANTHROPIC_DEFAULT_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Serialize)]
struct AnthropicRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: Vec<serde_json::Value>,
    messages: Vec<serde_json::Value>,
    tools: Vec<serde_json::Value>,
    stream: bool,
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
                    serde_json::json!({ "type": "tool_result", "tool_use_id": tool_use_id, "content": content }),

                ContentBlock::Image { media_type, data } =>
                    serde_json::json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": media_type, "data": data }
                    }),
            }
        }).collect();

        serde_json::json!({ "role": msg.role, "content": content })
    }).collect()
}

/// Per-content-block accumulator while parsing the SSE stream.
#[derive(Default)]
struct BlockAccum {
    kind: String,       // "text" or "tool_use"
    id: String,
    name: String,
    text: String,       // accumulated text (also streamed live to stdout)
    input_json: String, // accumulated tool input JSON
}

struct AnthropicClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
    url: String,
    suppress_stream: bool,
    bearer_auth: bool,
}

impl AnthropicClient {
    fn new(api_key: String, model: String, base_url: Option<String>, suppress_stream: bool) -> Self {
        let bearer_auth = base_url.is_some();
        // Use base_url exactly as provided — the gateway handles routing.
        // Fall back to the public Anthropic endpoint only when no base_url is set.
        let url = base_url
            .unwrap_or_else(|| ANTHROPIC_DEFAULT_URL.to_string());
        Self { http: crate::http::client().clone(), api_key, model, url, suppress_stream, bearer_auth }
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
                            if !suppress_stream {
                                if let Some(cb) = before_output.take() { cb(); }
                                highlighter.push(chunk);
                                let _ = std::io::Write::flush(&mut std::io::stdout());
                            }
                        }
                        acc.text.push_str(chunk);
                    } else if dtype == "input_json_delta" {
                        acc.input_json
                            .push_str(delta["partial_json"].as_str().unwrap_or(""));
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
    ) -> Result<ApiResponse> {
        let mut before_output = before_output;
        let mut highlighter = crate::stream_highlighter::StreamHighlighter::new();
        if self.api_key.is_empty() {
            anyhow::bail!("ANTHROPIC_API_KEY is not set");
        }

        // Build system prompt with cache_control breakpoint for prompt caching.
        // Anthropic caches system + tools, saving ~90% on repeated turns.
        let system_blocks = vec![serde_json::json!({
            "type": "text",
            "text": system,
            "cache_control": { "type": "ephemeral" }
        })];

        // Add cache_control to the last tool definition so the entire tool
        // block is cached as a unit.
        let mut cached_tools: Vec<serde_json::Value> = tools.to_vec();
        if let Some(last) = cached_tools.last_mut() {
            if let Some(obj) = last.as_object_mut() {
                obj.insert("cache_control".to_string(),
                    serde_json::json!({ "type": "ephemeral" }));
            }
        }

        let body = AnthropicRequest {
            model: &self.model,
            max_tokens: MAX_TOKENS,
            system: system_blocks,
            messages: encode_messages_anthropic(messages),
            tools: cached_tools,
            stream: true,
        };
        let body_bytes = serde_json::to_vec(&body).context("failed to serialize request")?;

        let api_key = self.api_key.clone();
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
            anyhow::bail!("Anthropic API returned {} (url: {}): {}", status, self.url, text);
        }

        // ── Parse SSE stream ──────────────────────────────────────────────────
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
                    if data == "[DONE]" {
                        break;
                    }
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

        // ── Assemble content blocks in index order ────────────────────────────
        let mut pairs: Vec<(usize, BlockAccum)> = blocks.into_iter().collect();
        pairs.sort_by_key(|(idx, _)| *idx);

        let had_text = pairs.iter().any(|(_, a)| a.kind == "text" && !a.text.is_empty());
        if had_text && !self.suppress_stream {
            highlighter.flush();
            // StreamHighlighter already printed newlines as needed.
        }

        let mut content: Vec<ContentBlock> = Vec::new();
        for (_, acc) in pairs {
            match acc.kind.as_str() {
                "text" if !acc.text.is_empty() => {
                    content.push(ContentBlock::Text { text: acc.text });
                }
                "tool_use" => {
                    let input: serde_json::Value =
                        serde_json::from_str(&acc.input_json).unwrap_or(serde_json::json!({}));
                    content.push(ContentBlock::ToolUse { id: acc.id, name: acc.name, input });
                }
                _ => {}
            }
        }

        // Stop spinner if no text was streamed (tool-use only response).
        if let Some(cb) = before_output.take() { cb(); }

        Ok(ApiResponse { content, stop_reason, usage: Some(usage_acc) })
    }
}

// ── OpenAI-compatible streaming client (OpenAI, LM Studio, Ollama) ───────────

const OPENAI_DEFAULT_BASE: &str = "https://api.openai.com";

/// Per-tool-call accumulator for the OpenAI SSE stream.
#[derive(Default)]
struct OaiToolAccum {
    id: String,
    name: String,
    arguments: String,
}

struct OpenAiClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
    url: String,
    suppress_stream: bool,
}

impl OpenAiClient {
    fn new(api_key: String, model: String, base_url: Option<String>, suppress_stream: bool) -> Self {
        // Use base_url exactly as provided — the gateway handles routing.
        // Fall back to the public OpenAI chat completions endpoint when not set.
        let url = base_url
            .unwrap_or_else(|| format!("{}/v1/chat/completions", OPENAI_DEFAULT_BASE));
        Self { http: crate::http::client().clone(), api_key, model, url, suppress_stream }
    }

    /// Convert our internal messages to the OpenAI wire format.
    fn encode_messages(system: &str, messages: &[Message]) -> Vec<serde_json::Value> {
        let mut out = vec![serde_json::json!({ "role": "system", "content": system })];

        for msg in messages {
            match msg.role.as_str() {
                "user" => {
                    let tool_results: Vec<&ContentBlock> = msg
                        .content
                        .iter()
                        .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
                        .collect();

                    if tool_results.is_empty() {
                        // May contain text + image blocks — build multimodal content array.
                        let parts: Vec<serde_json::Value> = msg.content.iter().filter_map(|b| {
                            match b {
                                ContentBlock::Text { text } =>
                                    Some(serde_json::json!({ "type": "text", "text": text })),
                                ContentBlock::Image { media_type, data } =>
                                    Some(serde_json::json!({
                                        "type": "image_url",
                                        "image_url": { "url": format!("data:{};base64,{}", media_type, data) }
                                    })),
                                _ => None,
                            }
                        }).collect();

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
                    out.push(m);
                }
                _ => {}
            }
        }
        out
    }

    /// Convert Anthropic-style tool definitions to OpenAI function format.
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
    ) -> Result<ApiResponse> {
        let mut before_output = before_output;
        let mut highlighter = crate::stream_highlighter::StreamHighlighter::new();
        let oai_messages = Self::encode_messages(system, messages);
        let oai_tools = Self::encode_tools(tools);

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": oai_messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if !oai_tools.is_empty() {
            body["tools"] = serde_json::json!(oai_tools);
        }
        let body_bytes = serde_json::to_vec(&body).context("failed to serialize request")?;

        let api_key = self.api_key.clone();
        let resp = send_with_retry(&self.http, |http| {
            let mut req = http
                .post(&self.url)
                .header("content-type", "application/json")
                .body(body_bytes.clone());
            if !api_key.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", api_key));
            }
            req
        })
        .await
        .context("failed to reach OpenAI-compatible API")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("OpenAI API returned {} (url: {}): {}", status, self.url, text);
        }

        // ── Parse SSE stream ──────────────────────────────────────────────────
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut text_acc = String::new();
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
                    if data == "[DONE]" {
                        break 'outer;
                    }
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                        let choice = &event["choices"][0];
                        let delta = &choice["delta"];

                        // Text delta — stream immediately (unless suppressed).
                        if let Some(text) = delta["content"].as_str() {
                            if !text.is_empty() {
                                if !self.suppress_stream {
                                    if let Some(cb) = before_output.take() { cb(); }
                                    highlighter.push(text);
                                    let _ = std::io::Write::flush(&mut std::io::stdout());
                                }
                                text_acc.push_str(text);
                            }
                        }

                        // Tool-call deltas — accumulate by index.
                        if let Some(tc_arr) = delta["tool_calls"].as_array() {
                            for tc in tc_arr {
                                let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                                let acc = tool_accums.entry(idx).or_default();
                                if let Some(id) = tc["id"].as_str() {
                                    acc.id = id.to_string();
                                }
                                if let Some(name) = tc["function"]["name"].as_str() {
                                    acc.name = name.to_string();
                                }
                                if let Some(args) = tc["function"]["arguments"].as_str() {
                                    acc.arguments.push_str(args);
                                }
                            }
                        }

                        // Finish reason.
                        if let Some(fr) = choice["finish_reason"].as_str() {
                            if !fr.is_empty() {
                                finish_reason = fr.to_string();
                            }
                        }

                        // Usage (arrives in the final chunk when stream_options.include_usage).
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

        // Stop spinner if no text was streamed.
        if let Some(cb) = before_output.take() { cb(); }

        // ── Assemble content blocks ───────────────────────────────────────────
        let mut content: Vec<ContentBlock> = Vec::new();
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

        Ok(ApiResponse { content, stop_reason, usage })
    }
}
