/// E2E tests for the /provider list and live Codex API connectivity.
use std::collections::HashMap;

use zap_coding_agent::config::{
    Config, OutputFormat, PermissionMode, Provider, ProviderEntry, SandboxMode, CODEX_CONTEXT_WINDOW,
};
use zap_coding_agent::session::configured_context_limit;
use zap_coding_agent::session::commands::provider::provider_slugs;

fn minimal_config(provider_slug: &str, model: &str) -> Config {
    Config {
        permission_mode: PermissionMode::Auto,
        sandbox: SandboxMode::Off,
        api_key: String::new(),
        model: model.to_string(),
        provider: Provider::OpenAi,
        base_url: None,
        output_format: OutputFormat::Text,
        agent_depth: 0,
        is_subagent: false,
        is_background_agent: false,
        spawn_depth: 0,
        proxy: None,
        no_proxy: None,
        ca_bundle: None,
        tls_skip_verify: false,
        timeout_secs: 120,
        budget: None,
        skill_paths: vec![],
        skill_token_budget: 4000,
        context_paths: vec![],
        allowed_paths: vec![],
        additional_dirs: vec![],
        disable_stream: false,
        skip_domain_prompt: false,
        tui_mode: false,
        tool_profile: "full".to_string(),
        provider_slug: provider_slug.to_string(),
        all_providers: HashMap::new(),
        disabled_tools: vec![],
        disabled_skills: vec![],
        model_routes: HashMap::new(),
        max_background_agents: 5,
    }
}

#[test]
fn codex_in_provider_list() {
    let slugs = provider_slugs();
    assert!(
        slugs.contains(&"codex"),
        "codex missing from provider list — got: {:?}",
        slugs
    );
}

#[test]
fn codex_after_claude_code() {
    let slugs = provider_slugs();
    let claude_pos = slugs.iter().position(|&s| s == "claude_code").expect("claude_code missing");
    let codex_pos  = slugs.iter().position(|&s| s == "codex").expect("codex missing");
    assert!(
        codex_pos == claude_pos + 1,
        "codex should immediately follow claude_code in the list (claude_code={}, codex={})",
        claude_pos, codex_pos
    );
}

#[test]
fn all_expected_providers_present() {
    let slugs = provider_slugs();
    for expected in &["lm_studio", "ollama", "anthropic", "claude_code", "codex",
                      "openai", "gemini", "deepseek", "groq", "mistral",
                      "xai", "together", "perplexity", "cohere",
                      "openrouter", "kimi", "zhipu", "qwen", "fireworks", "cerebras",
                      "opencode_zen", "yolo-auto", "custom"] {
        assert!(slugs.contains(expected), "provider '{}' missing from list", expected);
    }
    assert_eq!(slugs.len(), 23, "expected 23 providers, got {}", slugs.len());
}

// Both scenarios live in one test (rather than two `#[test]` fns) because
// they mutate the process-wide CODEX_HOME env var — split across two tests,
// cargo's parallel test runner raced them and flaked the push hook
// (whichever test's remove_var() landed between the other's set_var() and
// its check_codex() call would see the wrong state).
#[test]
fn check_codex_auth_presence() {
    let absent_dir = std::env::temp_dir().join("zap_codex_test_no_auth");
    let _ = std::fs::create_dir_all(&absent_dir);
    std::env::set_var("CODEX_HOME", &absent_dir);
    let result = zap_coding_agent::llm_client::auth::check_codex();
    assert!(result.is_none(), "check_codex() should return None when auth.json absent");

    let present_dir = std::env::temp_dir().join("zap_codex_test_with_auth");
    let _ = std::fs::create_dir_all(&present_dir);
    std::fs::write(
        present_dir.join("auth.json"),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"tok","account_id":"acc"}}"#,
    ).unwrap();
    std::env::set_var("CODEX_HOME", &present_dir);
    let result = zap_coding_agent::llm_client::auth::check_codex();
    std::env::remove_var("CODEX_HOME");
    let _ = std::fs::remove_dir_all(&present_dir);
    assert_eq!(result, Some("ready".into()), "check_codex() should return Some when auth.json present");
}

#[test]
fn codex_context_window_contract_end_to_end() {
    let slugs = provider_slugs();
    assert!(slugs.contains(&"codex"), "codex provider missing from /provider list");

    for model in ["gpt-5.5", "gpt-5.5-codex", "future-codex-model"] {
        let mut config = minimal_config("codex", model);
        config.all_providers.insert("codex".to_string(), ProviderEntry {
            kind: Some("openai".to_string()),
            model: Some(model.to_string()),
            ..Default::default()
        });

        assert_eq!(
            configured_context_limit(&config),
            CODEX_CONTEXT_WINDOW,
            "Codex provider must use the Codex context window regardless of model name"
        );
    }

    let mut custom_codex = minimal_config("custom_codex", "gpt-5.5");
    custom_codex.all_providers.insert("custom_codex".to_string(), ProviderEntry {
        kind: Some("codex".to_string()),
        model: Some("gpt-5.5".to_string()),
        ..Default::default()
    });
    assert_eq!(configured_context_limit(&custom_codex), CODEX_CONTEXT_WINDOW);

    let mut explicit_override = custom_codex.clone();
    explicit_override.all_providers.insert("custom_codex".to_string(), ProviderEntry {
        kind: Some("codex".to_string()),
        model: Some("gpt-5.5".to_string()),
        context_window: Some(123_456),
        ..Default::default()
    });
    assert_eq!(
        configured_context_limit(&explicit_override),
        123_456,
        "explicit provider context_window should still override the Codex default"
    );
}

// ── OpenRouter helpers ────────────────────────────────────────────────────────

fn openrouter_key() -> String {
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        return key;
    }
    let toml_path = dirs::home_dir().expect("no home").join(".agent.toml");
    let raw = std::fs::read_to_string(&toml_path)
        .expect("~/.agent.toml missing; set OPENROUTER_API_KEY env var to run these tests");
    let mut in_section = false;
    for line in raw.lines() {
        let t = line.trim();
        if t == "[providers.openrouter]" { in_section = true; continue; }
        if in_section && t.starts_with('[') { break; }
        if in_section && t.starts_with("api_key") {
            if let Some(val) = t.split('"').nth(1) {
                return val.to_owned();
            }
        }
    }
    panic!("openrouter api_key not found in ~/.agent.toml; set OPENROUTER_API_KEY env var");
}

fn openrouter_post(model: &str) -> (u16, String) {
    let key = openrouter_key();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "model": model,
            "messages": [{"role": "user", "content": "Reply with the single word: pong"}],
        });
        let resp = client
            .post("https://openrouter.ai/api/v1/chat/completions")
            .header("Authorization", format!("Bearer {key}"))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .expect("HTTP request failed");
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        (status, body)
    })
}

fn assert_openrouter_ok(model: &str) {
    let (status, body) = openrouter_post(model);
    println!("{model} → HTTP {status}");
    println!("{}", &body[..body.len().min(500)]);
    assert!((200..300).contains(&status), "{model} returned {status}: {body}");
    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("{model}: response is not JSON: {body}"));
    let msg = &json["choices"][0]["message"];
    // reasoning models may return content=null with reasoning_content populated
    assert!(
        msg["content"].is_string() || msg["reasoning_content"].is_string(),
        "{model}: no content or reasoning_content in response: {body}"
    );
}

// ── OpenRouter: free models ($0) ──────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter free ($0): meta-llama/llama-3.3-70b-instruct:free"]
fn openrouter_free_llama_3_3_70b() {
    assert_openrouter_ok("meta-llama/llama-3.3-70b-instruct:free");
}

#[test]
#[ignore = "live — OpenRouter free ($0): google/gemma-4-26b-a4b-it:free"]
fn openrouter_free_gemma_4_26b() {
    assert_openrouter_ok("google/gemma-4-26b-a4b-it:free");
}

#[test]
#[ignore = "live — OpenRouter free ($0): qwen/qwen3-coder:free"]
fn openrouter_free_qwen3_coder() {
    assert_openrouter_ok("qwen/qwen3-coder:free");
}

#[test]
#[ignore = "live — OpenRouter free ($0): nvidia/nemotron-3-super-120b-a12b:free"]
fn openrouter_free_nemotron_super_120b() {
    assert_openrouter_ok("nvidia/nemotron-3-super-120b-a12b:free");
}

#[test]
#[ignore = "live — OpenRouter free ($0): openai/gpt-oss-120b:free"]
fn openrouter_free_gpt_oss_120b() {
    assert_openrouter_ok("openai/gpt-oss-120b:free");
}

// ── OpenRouter: Anthropic Claude ──────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter paid ($1/$5 per M): anthropic/claude-haiku-4.5"]
fn openrouter_anthropic_claude_haiku_4_5() {
    assert_openrouter_ok("anthropic/claude-haiku-4.5");
}

#[test]
#[ignore = "live — OpenRouter paid ($3/$15 per M): anthropic/claude-sonnet-4.6"]
fn openrouter_anthropic_claude_sonnet_4_6() {
    assert_openrouter_ok("anthropic/claude-sonnet-4.6");
}

#[test]
#[ignore = "live — OpenRouter paid ($5/$25 per M): anthropic/claude-opus-4.8"]
fn openrouter_anthropic_claude_opus_4_8() {
    assert_openrouter_ok("anthropic/claude-opus-4.8");
}

// ── OpenRouter: OpenAI ────────────────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter paid ($0.10/$0.40 per M): openai/gpt-4.1-nano"]
fn openrouter_openai_gpt41_nano() {
    assert_openrouter_ok("openai/gpt-4.1-nano");
}

#[test]
#[ignore = "live — OpenRouter paid ($0.40/$1.60 per M): openai/gpt-4.1-mini"]
fn openrouter_openai_gpt41_mini() {
    assert_openrouter_ok("openai/gpt-4.1-mini");
}

#[test]
#[ignore = "live — OpenRouter paid ($0.15/$0.60 per M): openai/gpt-4o-mini"]
fn openrouter_openai_gpt4o_mini() {
    assert_openrouter_ok("openai/gpt-4o-mini");
}

#[test]
#[ignore = "live — OpenRouter paid ($2.00/$8.00 per M): openai/gpt-4.1"]
fn openrouter_openai_gpt41() {
    assert_openrouter_ok("openai/gpt-4.1");
}

// ── OpenRouter: Meta Llama ────────────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter paid ($0.10/$0.32 per M): meta-llama/llama-3.3-70b-instruct"]
fn openrouter_llama_3_3_70b() {
    assert_openrouter_ok("meta-llama/llama-3.3-70b-instruct");
}

#[test]
#[ignore = "live — OpenRouter paid ($0.15/$0.60 per M): meta-llama/llama-4-maverick"]
fn openrouter_llama_4_maverick() {
    assert_openrouter_ok("meta-llama/llama-4-maverick");
}

#[test]
#[ignore = "live — OpenRouter paid ($0.10/$0.30 per M): meta-llama/llama-4-scout"]
fn openrouter_llama_4_scout() {
    assert_openrouter_ok("meta-llama/llama-4-scout");
}

// ── OpenRouter: Google Gemini ─────────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter paid ($0.10/$0.40 per M): google/gemini-2.5-flash-lite"]
fn openrouter_google_gemini_25_flash_lite() {
    assert_openrouter_ok("google/gemini-2.5-flash-lite");
}

#[test]
#[ignore = "live — OpenRouter paid ($0.30/$2.50 per M): google/gemini-2.5-flash"]
fn openrouter_google_gemini_25_flash() {
    assert_openrouter_ok("google/gemini-2.5-flash");
}

#[test]
#[ignore = "live — OpenRouter paid ($1.25/$10.00 per M): google/gemini-2.5-pro"]
fn openrouter_google_gemini_25_pro() {
    assert_openrouter_ok("google/gemini-2.5-pro");
}

// ── OpenRouter: DeepSeek ──────────────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter paid ($0.20/$0.80 per M): deepseek/deepseek-chat"]
fn openrouter_deepseek_chat() {
    assert_openrouter_ok("deepseek/deepseek-chat");
}

#[test]
#[ignore = "live — OpenRouter paid ($0.70/$2.50 per M): deepseek/deepseek-r1 (reasoning)"]
fn openrouter_deepseek_r1() {
    assert_openrouter_ok("deepseek/deepseek-r1");
}

// ── OpenRouter: Qwen ──────────────────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter paid ($0.05/$0.40 per M): qwen/qwen3-8b"]
fn openrouter_qwen3_8b() {
    assert_openrouter_ok("qwen/qwen3-8b");
}

#[test]
#[ignore = "live — OpenRouter paid ($0.08/$0.28 per M): qwen/qwen3-32b"]
fn openrouter_qwen3_32b() {
    assert_openrouter_ok("qwen/qwen3-32b");
}

#[test]
#[ignore = "live — OpenRouter paid ($0.45/$1.82 per M): qwen/qwen3-235b-a22b"]
fn openrouter_qwen3_235b() {
    assert_openrouter_ok("qwen/qwen3-235b-a22b");
}

// ── OpenRouter: Mistral ───────────────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter paid ($0.02/$0.03 per M): mistralai/mistral-nemo (cheapest Mistral)"]
fn openrouter_mistral_nemo() {
    assert_openrouter_ok("mistralai/mistral-nemo");
}

#[test]
#[ignore = "live — OpenRouter paid ($0.07/$0.20 per M): mistralai/mistral-small-3.2-24b-instruct"]
fn openrouter_mistral_small_3_2() {
    assert_openrouter_ok("mistralai/mistral-small-3.2-24b-instruct");
}

#[test]
#[ignore = "live — OpenRouter paid ($0.50/$1.50 per M): mistralai/mistral-large-2512"]
fn openrouter_mistral_large_2512() {
    assert_openrouter_ok("mistralai/mistral-large-2512");
}

// ── OpenRouter: xAI Grok ──────────────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter paid ($1.25/$2.50 per M): x-ai/grok-4.20"]
fn openrouter_xai_grok_4_20() {
    assert_openrouter_ok("x-ai/grok-4.20");
}

// ── OpenRouter: Amazon Nova ───────────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter paid ($0.06/$0.24 per M): amazon/nova-lite-v1"]
fn openrouter_amazon_nova_lite() {
    assert_openrouter_ok("amazon/nova-lite-v1");
}

#[test]
#[ignore = "live — OpenRouter paid ($0.80/$3.20 per M): amazon/nova-pro-v1"]
fn openrouter_amazon_nova_pro() {
    assert_openrouter_ok("amazon/nova-pro-v1");
}

// ── OpenRouter: Cohere ────────────────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter paid ($0.04/$0.15 per M): cohere/command-r7b-12-2024"]
fn openrouter_cohere_command_r7b() {
    assert_openrouter_ok("cohere/command-r7b-12-2024");
}

// ── OpenRouter: Perplexity ────────────────────────────────────────────────────

#[test]
#[ignore = "live — OpenRouter paid ($1.00/$1.00 per M): perplexity/sonar"]
fn openrouter_perplexity_sonar() {
    assert_openrouter_ok("perplexity/sonar");
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn codex_creds() -> (String, String) {
    let auth_path = dirs::home_dir()
        .expect("no home dir")
        .join(".codex/auth.json");
    assert!(auth_path.exists(), "~/.codex/auth.json not found — run `codex login` first");
    let raw = std::fs::read_to_string(&auth_path).expect("failed to read auth.json");
    let data: serde_json::Value = serde_json::from_str(&raw).expect("auth.json is not valid JSON");
    let token = data["tokens"]["access_token"].as_str()
        .expect("tokens.access_token missing").to_owned();
    let account = data["tokens"]["account_id"].as_str()
        .expect("tokens.account_id missing").to_owned();
    (token, account)
}

fn codex_post(token: &str, account: &str, model: &str) -> (u16, String) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "model":        model,
            "input":        [{"role": "user", "content": "Reply with the single word: pong"}],
            "instructions": "You are a test echo bot. Follow instructions exactly.",
            "store":        false,
            "stream":       true,
        });
        let resp = client
            .post("https://chatgpt.com/backend-api/codex/responses")
            .header("Authorization", format!("Bearer {token}"))
            .header("ChatGPT-Account-ID", account)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .expect("HTTP request failed");
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        (status, body)
    })
}

// ── live model tests ──────────────────────────────────────────────────────────

/// Live network test — hits the real Codex Responses API.
/// Skipped by default; run explicitly with: cargo test -- --ignored
#[test]
#[ignore = "live network call — requires ~/.codex/auth.json with a valid token"]
fn codex_live_api_o4_mini() {
    let (token, account) = codex_creds();
    let (status, body) = codex_post(&token, &account, "gpt-5.5");

    println!("Codex API status : {status}");
    println!("Codex API body   : {}", &body[..body.len().min(800)]);

    assert!((200..300).contains(&status), "Expected 2xx from Codex API, got {status}: {body}");
    assert!(!body.is_empty(), "Response body was empty");
    // SSE stream — must not start with an error JSON blob.
    assert!(!body.starts_with("{\"detail\""), "Codex API returned error: {body}");
}

#[test]
#[ignore = "live network call — documents that gpt-5.4 is NOT supported via ChatGPT account"]
fn codex_live_api_gpt54_unsupported() {
    let (token, account) = codex_creds();
    let (status, body) = codex_post(&token, &account, "gpt-5.4");

    println!("gpt-5.4 status : {status}");
    println!("gpt-5.4 body   : {body}");

    assert_eq!(status, 400, "Expected 400 for unsupported model gpt-5.4, got {status}");
    assert!(
        body.contains("not supported"),
        "Expected 'not supported' in error, got: {body}"
    );
}

#[test]
#[ignore = "live network call — documents that gpt-5.3-codex is NOT supported via ChatGPT account"]
fn codex_live_api_gpt53_unsupported() {
    let (token, account) = codex_creds();
    let (status, body) = codex_post(&token, &account, "gpt-5.3-codex");

    println!("gpt-5.3-codex status : {status}");
    println!("gpt-5.3-codex body   : {body}");

    assert_eq!(status, 400, "Expected 400 for unsupported model gpt-5.3-codex, got {status}");
    assert!(
        body.contains("not supported"),
        "Expected 'not supported' in error, got: {body}"
    );
}
