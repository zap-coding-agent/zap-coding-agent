use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};

use crate::config::{Config, OutputFormat, Provider};

const MAX_TOKENS: u32 = 16_000;
const MAX_RETRIES: u32 = 5;

/// Redact an API key for log files: show first 4 + last 4 chars and the total
/// length so you can distinguish keys without exposing usable credentials.
fn redact_token(token: &str) -> String {
    if token.is_empty() { return "(none)".to_string(); }
    if token.len() <= 8  { return "***".to_string(); }
    format!("{}…{} ({} chars)", &token[..4], &token[token.len()-4..], token.len())
}

/// Persist the request body as a standalone JSON file and return a ready-to-run
/// curl command that references it.  The body has `stream` forced to false so
/// the curl response is a plain JSON object rather than an SSE stream.
///
/// The curl command contains the **real** API key — `llm_requests/` and
/// `llm.log` should be treated as sensitive local debug files.
fn build_curl_block(
    slug: &str,          // e.g. "openai" or "anthropic"
    url: &str,
    auth_header: &str,   // e.g. "Authorization" or "x-api-key"
    auth_value: &str,    // real (unredacted) token
    body_value: &serde_json::Value,
) -> String {
    // Force stream:false — reading SSE in a terminal is painful.
    let mut curl_body = body_value.clone();
    if let Some(obj) = curl_body.as_object_mut() {
        obj.insert("stream".to_string(), serde_json::Value::Bool(false));
        obj.remove("stream_options"); // only meaningful with streaming
    }
    let compact = serde_json::to_string(&curl_body).unwrap_or_default();

    // Save body to ~/.zap/llm_requests/ so the curl command stays short.
    let body_path = crate::log::save_request_body(slug, &compact);

    match body_path {
        Some(p) => {
            // Use forward slashes so the path works in bash on all platforms
            // (Windows PathBuf::display() emits backslashes which Git Bash rejects).
            let path_str = p.to_string_lossy().replace('\\', "/");
            format!(
                "\n# curl (body: {path} — contains real key, treat as sensitive):\n\
                 curl -s '{url}' \\\n\
                 \x20 -H 'Content-Type: application/json' \\\n\
                 \x20 -H '{auth_header}: {auth_value}' \\\n\
                 \x20 -d @'{path}'",
                path = path_str, url = url,
                auth_header = auth_header, auth_value = auth_value,
            )
        }
        None => format!(
            "\n# curl (could not save body file):\n\
             curl -s '{url}' -H '{auth_header}: {auth_value}' -H 'Content-Type: application/json' \\\n\
             \x20 -d '<see body above, change stream to false>'",
            url = url, auth_header = auth_header, auth_value = auth_value,
        ),
    }
}

/// Detect whether a non-2xx error body suggests the gateway does not support
/// the OpenAI tools / function-calling API.
fn check_tool_support_error(status: u16, body: &str) {
    if status != 400 && status != 422 { return; }
    let lower = body.to_lowercase();
    if lower.contains("tool") || lower.contains("function") || lower.contains("function_call") {
        crate::zap_warn!(
            "Gateway returned HTTP {status} with a tool/function-related error. \
             The endpoint at your base_url may not support the OpenAI tools API. \
             Consider using a model or gateway that supports function calling, \
             or contact your IT team. Error snippet: {}",
            body.chars().take(300).collect::<String>()
        );
    }
}

/// Detect whether a model that received tools is responding with tool-call JSON
/// embedded in plain text — a sign the gateway silently stripped the tools array.
fn check_text_mode_tool_call(text: &str, tools_were_sent: bool) {
    if !tools_were_sent || text.is_empty() { return; }
    // Pattern: {"name": "...", "arguments": ...} or similar JSON tool-call blobs in text.
    let has_name_field = text.contains(r#""name":"#) || text.contains(r#""name": "#);
    let has_args_field = text.contains(r#""arguments":"#) || text.contains(r#""arguments": "#)
                      || text.contains(r#""parameters":"#) || text.contains(r#""input":"#);
    if has_name_field && has_args_field {
        crate::zap_warn!(
            "Model appears to be outputting tool calls as plain text instead of using \
             the native function-calling API. Your gateway may be silently stripping \
             the 'tools' field from requests. Check your base_url and gateway settings."
        );
    }
}

// ── Shared types (internal representation) ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String },
    /// Base64-encoded image (jpeg, png, gif, webp). Sent via /attach.
    Image { media_type: String, data: String },
    /// Anthropic extended-thinking block. `signature` is opaque — required
    /// when echoing the block back for multi-turn continuations.
    Thinking { thinking: String, signature: String },
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
        // thinking_budget: 0 = disabled; Anthropic uses it; OpenAI providers ignore it
        thinking_budget: u32,
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
            config.disable_stream,
        )),
        Provider::OpenAi => Box::new(OpenAiClient::new(
            config.api_key.clone(),
            config.model.clone(),
            config.base_url.clone(),
            suppress,
            config.disable_stream,
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
                    serde_json::json!({ "type": "tool_result", "tool_use_id": tool_use_id, "content": content }),

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
            }
        }).collect();

        serde_json::json!({ "role": msg.role, "content": content })
    }).collect()
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

struct AnthropicClient {
    http: reqwest::Client,
    api_key: String,
    model: String,
    url: String,
    suppress_stream: bool,
    bearer_auth: bool,
    disable_stream: bool,
}

impl AnthropicClient {
    fn new(api_key: String, model: String, base_url: Option<String>, suppress_stream: bool, disable_stream: bool) -> Self {
        let bearer_auth = base_url.is_some();
        let url = base_url
            .unwrap_or_else(|| ANTHROPIC_DEFAULT_URL.to_string());
        Self { http: crate::http::client().clone(), api_key, model, url, suppress_stream, bearer_auth, disable_stream }
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
                            // Always forward to remote control (no-op if inactive).
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
                            // Emit to TUI if in TUI mode; never print to stdout.
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

        // budget_tokens must be strictly less than max_tokens per Anthropic's API.
        let effective_budget = thinking_budget.min(MAX_TOKENS.saturating_sub(1));
        let thinking = if effective_budget > 0 {
            Some(serde_json::json!({"type": "enabled", "budget_tokens": effective_budget}))
        } else {
            None
        };
        let body = AnthropicRequest {
            model: &self.model,
            max_tokens: MAX_TOKENS,
            system: system_blocks,
            messages: encode_messages_anthropic(messages),
            tools: cached_tools,
            stream: !self.disable_stream,
            thinking,
        };
        let body_bytes = serde_json::to_vec(&body).context("failed to serialize request")?;

        // Log request — replace tools array with a count to keep the log readable.
        if let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
            let n = v["tools"].as_array().map(|t| t.len()).unwrap_or(0);

            // Curl block uses the real body (tool schemas included).
            let (auth_hdr, auth_val) = if self.bearer_auth {
                ("Authorization", format!("Bearer {}", self.api_key))
            } else {
                ("x-api-key", self.api_key.clone())
            };
            let curl = build_curl_block("anthropic", &self.url, auth_hdr, &auth_val, &v);

            // Pretty log: redact token and collapse tools to a count.
            v["tools"] = serde_json::json!(format!("<{n} tools — omitted>"));
            if let Ok(pretty) = serde_json::to_string_pretty(&v) {
                let auth_line = format!("{}: {}", auth_hdr, redact_token(&auth_val));
                crate::log::write_llm(
                    "REQUEST [anthropic]",
                    &format!("POST {}\n{}\n\n{}{}", self.url, auth_line, pretty, curl),
                );
            }
        }

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
            crate::log::write_llm("ERROR [anthropic]", &format!("HTTP {} — {}", status, text));
            check_tool_support_error(status.as_u16(), &text);
            anyhow::bail!("Anthropic API returned {} (url: {}): {}", status, self.url, text);
        }

        // ── Parse response: plain JSON (disable_stream) or SSE ───────────────
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
            // ── SSE streaming path ────────────────────────────────────────────
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

        // Log response to ~/.zap/llm.log
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
                }).collect::<Vec<_>>(),
            });
            if let Ok(pretty) = serde_json::to_string_pretty(&resp_val) {
                crate::log::write_llm("RESPONSE [anthropic]", &pretty);
            }
        }

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
    disable_stream: bool,
    /// Some OpenAI-compatible providers (e.g. DeepSeek) don't support image content.
    /// When true, image blocks are dropped with a warning rather than sent as
    /// `image_url` (which would cause a 400 error).
    image_support: bool,
}

impl OpenAiClient {
    fn new(api_key: String, model: String, base_url: Option<String>, suppress_stream: bool, disable_stream: bool) -> Self {
        let url = base_url
            .unwrap_or_else(|| format!("{}/v1/chat/completions", OPENAI_DEFAULT_BASE));
        // Detect providers known to lack vision support.
        let image_support = !url.contains("deepseek.com");
        Self { http: crate::http::client().clone(), api_key, model, url, suppress_stream, disable_stream, image_support }
    }

    /// Convert our internal messages to the OpenAI wire format.
    fn encode_messages(&self, system: &str, messages: &[Message]) -> Vec<serde_json::Value> {
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

                        // Warn only when image blocks were dropped.
                        if !self.image_support && parts.len() < msg.content.len() {
                            let dropped = msg.content.len() - parts.len();
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
        _thinking_budget: u32,  // OpenAI-compatible providers don't support extended thinking
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

        // Log request — replace tools array with a count to keep the log readable.
        let tools_were_sent = !oai_tools.is_empty();
        if let Ok(mut v) = serde_json::from_slice::<serde_json::Value>(&body_bytes) {
            let n = v["tools"].as_array().map(|t| t.len()).unwrap_or(0);

            // Curl block uses the real body and real key.
            let auth_val = if self.api_key.is_empty() {
                String::new()
            } else {
                format!("Bearer {}", self.api_key)
            };
            let curl = if auth_val.is_empty() {
                build_curl_block("openai", &self.url, "Authorization", "", &v)
            } else {
                build_curl_block("openai", &self.url, "Authorization", &auth_val, &v)
            };

            // Pretty log: redact token and collapse tools to a count.
            if n > 0 {
                v["tools"] = serde_json::json!(format!("<{n} tools — omitted>"));
            }
            if let Ok(pretty) = serde_json::to_string_pretty(&v) {
                let auth_line = if auth_val.is_empty() {
                    "(no auth)".to_string()
                } else {
                    format!("Authorization: {}", redact_token(&auth_val))
                };
                crate::log::write_llm(
                    "REQUEST [openai]",
                    &format!("POST {}\n{}\n\n{}{}", self.url, auth_line, pretty, curl),
                );
            }
        }

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
            crate::log::write_llm("ERROR [openai]", &format!("HTTP {} — {}", status, text));
            check_tool_support_error(status.as_u16(), &text);
            anyhow::bail!("OpenAI API returned {} (url: {}): {}", status, self.url, text);
        }

        // ── Parse response: plain JSON (disable_stream) or SSE ───────────────
        let (content, stop_reason, usage) = if self.disable_stream {
            let text = resp.text().await.context("failed to read OpenAI response")?;
            let json: serde_json::Value = serde_json::from_str(&text)
                .context("failed to parse OpenAI JSON response")?;

            let choice = &json["choices"][0];
            let message = &choice["message"];
            let finish_reason = choice["finish_reason"].as_str().unwrap_or("stop").to_string();

            let mut content: Vec<ContentBlock> = Vec::new();
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

            // Plain-text tool-call detection still applies.
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
            // ── SSE streaming path ────────────────────────────────────────────
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
                        if data == "[DONE]" { break 'outer; }
                        if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                            let choice = &event["choices"][0];
                            let delta = &choice["delta"];

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

        // Log response to ~/.zap/llm.log
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
                }).collect::<Vec<_>>(),
            });
            if let Ok(pretty) = serde_json::to_string_pretty(&resp_val) {
                crate::log::write_llm("RESPONSE [openai]", &pretty);
            }
        }

        Ok(ApiResponse { content, stop_reason, usage })
    }
}
