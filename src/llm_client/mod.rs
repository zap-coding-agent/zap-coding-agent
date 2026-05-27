pub mod anthropic;
pub mod openai;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::{Config, OutputFormat, Provider};

use anthropic::AnthropicClient;
use openai::OpenAiClient;

const MAX_RETRIES: u32 = 5;
const ANTHROPIC_DEFAULT_URL: &str = "https://api.anthropic.com/v1/messages";
const OPENAI_DEFAULT_BASE: &str = "https://api.openai.com";

/// Returns false for providers known to reject image content blocks.
pub fn provider_supports_vision(config: &Config) -> bool {
    match config.provider {
        Provider::Anthropic => true,
        Provider::OpenAi => {
            let url = config.base_url.as_deref().unwrap_or("");
            !url.contains("deepseek.com")
        }
    }
}

fn redact_token(token: &str) -> String {
    if token.is_empty() { return "(none)".to_string(); }
    if token.len() <= 8  { return "***".to_string(); }
    format!("{}…{} ({} chars)", &token[..4], &token[token.len()-4..], token.len())
}

fn build_curl_block(
    slug: &str,
    url: &str,
    auth_header: &str,
    auth_value: &str,
    body_value: &serde_json::Value,
) -> String {
    let mut curl_body = body_value.clone();
    if let Some(obj) = curl_body.as_object_mut() {
        obj.insert("stream".to_string(), serde_json::Value::Bool(false));
        obj.remove("stream_options");
    }
    let compact = serde_json::to_string(&curl_body).unwrap_or_default();
    let body_path = crate::log::save_request_body(slug, &compact);

    match body_path {
        Some(p) => {
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

fn check_text_mode_tool_call(text: &str, tools_were_sent: bool) {
    if !tools_were_sent || text.is_empty() { return; }
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

// ── Shared types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: serde_json::Value },
    ToolResult { tool_use_id: String, content: String },
    Image { media_type: String, data: String },
    Thinking { thinking: String, signature: String },
    Reasoning { content: String },
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

pub type BeforeOutput = Box<dyn FnOnce() + Send>;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn send(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[serde_json::Value],
        before_output: Option<BeforeOutput>,
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

pub(super) async fn send_with_retry(
    http: &reqwest::Client,
    build: impl Fn(&reqwest::Client) -> reqwest::RequestBuilder,
) -> Result<reqwest::Response> {
    let mut last_resp = None;
    for attempt in 0..MAX_RETRIES {
        let resp = build(http).send().await?;
        let status = resp.status().as_u16();
        let retryable = status == 429 || status == 503 || status == 502;
        if !retryable {
            return Ok(resp);
        }
        let delay_ms: u64 = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(|s| s * 1_000)
            .unwrap_or(5_000 << attempt);
        let remaining = MAX_RETRIES - attempt - 1;
        if remaining > 0 {
            let reason = if status == 429 { "rate limited" } else { "service unavailable" };
            let msg = format!("  ⚠ {reason} (HTTP {status}) — retrying in {}s… ({remaining} attempt(s) left)",
                delay_ms / 1_000);
            if crate::tui::channel::is_tui_mode() {
                crate::tui::channel::tui_send(crate::tui::channel::TuiEvent::LlmChunk(format!("\n{msg}")));
            } else {
                println!("{msg}");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
        } else {
            last_resp = Some(resp);
        }
    }
    Ok(last_resp.unwrap())
}

// ── URL normalisation helpers ─────────────────────────────────────────────────

pub fn normalize_anthropic_url(base_url: Option<&str>) -> String {
    match base_url {
        Some(u) => {
            let u = u.trim_end_matches('/');
            if u.ends_with("/messages") { u.to_string() }
            else { format!("{}/v1/messages", u) }
        }
        None => ANTHROPIC_DEFAULT_URL.to_string(),
    }
}

pub fn normalize_openai_url(base_url: Option<&str>) -> String {
    match base_url {
        Some(u) => {
            let u = u.trim_end_matches('/');
            if u.ends_with("/chat/completions") { u.to_string() }
            else if u.ends_with("/v1") { format!("{}/chat/completions", u) }
            else { format!("{}/v1/chat/completions", u) }
        }
        None => format!("{}/v1/chat/completions", OPENAI_DEFAULT_BASE),
    }
}

#[cfg(test)]
mod url_tests {
    use super::{normalize_openai_url, normalize_anthropic_url, ANTHROPIC_DEFAULT_URL, OPENAI_DEFAULT_BASE};

    #[test]
    fn openai_full_endpoint_used_as_is() {
        assert_eq!(
            normalize_openai_url(Some("https://api.deepseek.com/v1/chat/completions")),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn openai_base_url_gets_path_appended() {
        assert_eq!(
            normalize_openai_url(Some("https://api.deepseek.com")),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn openai_trailing_slash_trimmed() {
        assert_eq!(
            normalize_openai_url(Some("https://api.groq.com/openai/v1/chat/completions/")),
            "https://api.groq.com/openai/v1/chat/completions"
        );
    }

    #[test]
    fn openai_v1_base_gets_path_appended() {
        assert_eq!(
            normalize_openai_url(Some("https://api.mistral.ai/v1")),
            "https://api.mistral.ai/v1/chat/completions"
        );
    }

    #[test]
    fn openai_lm_studio_full_url() {
        assert_eq!(
            normalize_openai_url(Some("http://localhost:1234/v1/chat/completions")),
            "http://localhost:1234/v1/chat/completions"
        );
    }

    #[test]
    fn openai_none_uses_default() {
        assert_eq!(
            normalize_openai_url(None),
            format!("{}/v1/chat/completions", OPENAI_DEFAULT_BASE)
        );
    }

    #[test]
    fn anthropic_full_endpoint_used_as_is() {
        assert_eq!(
            normalize_anthropic_url(Some("https://my-gateway.corp/v1/messages")),
            "https://my-gateway.corp/v1/messages"
        );
    }

    #[test]
    fn anthropic_base_url_gets_path_appended() {
        assert_eq!(
            normalize_anthropic_url(Some("https://my-gateway.corp")),
            "https://my-gateway.corp/v1/messages"
        );
    }

    #[test]
    fn anthropic_trailing_slash_trimmed() {
        assert_eq!(
            normalize_anthropic_url(Some("https://my-gateway.corp/v1/messages/")),
            "https://my-gateway.corp/v1/messages"
        );
    }

    #[test]
    fn anthropic_none_uses_default() {
        assert_eq!(normalize_anthropic_url(None), ANTHROPIC_DEFAULT_URL);
    }
}
