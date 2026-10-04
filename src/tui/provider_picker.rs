use super::app::{App, ProviderEntry, ProviderKind, ProviderPickerState};
use crate::config::Config;

/// Return the selectable model list for the currently active provider.
/// Priority: live-fetch (local/gomodel) → TOML models map → static fallback per slug.
pub(crate) fn models_for_current_provider(config: &Config) -> Vec<String> {
    let slug = &config.provider_slug;
    if let Some(entry) = config.all_providers.get(slug) {
        if let Some(ref url) = entry.base_url {
            let fetched = crate::llm_client::fetch_openai_compatible_models_with_auth(
                url, entry.api_key.as_deref(), &entry.extra_headers,
            );
            if !fetched.is_empty() {
                let mut m = fetched;
                m.push("Other…".into());
                return m;
            }
        }
        let mut m: Vec<String> = entry.models.keys().cloned().collect();
        m.sort();
        if !m.is_empty() {
            m.push("Other…".into());
            return m;
        }
        if let Some(ref model) = entry.model {
            return vec![model.clone(), "Other…".into()];
        }
    }
    // Static fallback keyed by slug
    let mut m: Vec<String> = match slug.as_str() {
        "anthropic"    => vec!["claude-fable-5", "claude-sonnet-4-6", "claude-opus-4-8", "claude-haiku-4-5-20251001"],
        "claude_code"  => vec!["auto", "claude-fable-5", "claude-sonnet-4-6", "claude-opus-4-8", "claude-haiku-4-5-20251001"],
        "openai"       => vec!["gpt-4o", "gpt-4o-mini", "o3", "o4-mini"],
        "codex"        => vec!["gpt-5.5", "gpt-5.4", "gpt-5", "gpt-4.1", "o4-mini", "o3", "gpt-4o"],
        "gemini"       => vec!["gemini-2.0-flash", "gemini-2.5-pro", "gemini-2.5-flash"],
        "deepseek"     => vec!["deepseek-v4-pro", "deepseek-v4-flash", "deepseek-chat", "deepseek-reasoner"],
        "groq"         => vec!["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "mixtral-8x7b-32768"],
        "mistral"      => vec!["mistral-large-latest", "codestral-latest", "mistral-small-latest"],
        "xai"          => vec!["grok-3", "grok-3-mini", "grok-2"],
        "together"     => vec!["meta-llama/Llama-3-70b-chat-hf", "Qwen/Qwen2.5-72B-Instruct-Turbo"],
        "perplexity"   => vec!["sonar-pro", "sonar", "sonar-reasoning"],
        "cohere"       => vec!["command-a-03-2025", "command-r7b-12-2024"],
        "openrouter"   => vec!["anthropic/claude-opus-4.8", "anthropic/claude-sonnet-4.6", "openai/gpt-4.1", "google/gemini-2.5-pro", "deepseek/deepseek-r1"],
        "lm_studio"    => vec!["qwen3-coder-30b", "devstral-small-2", "gemma-4-e4b"],
        "ollama"       => vec!["llama3.2", "llama3.1:70b", "codellama", "qwen2.5-coder"],
        "cerebras"     => vec!["gpt-oss-120b", "zai-glm-4.7"],
        "opencode_zen" => vec!["grok-4.5", "glm-5.2", "glm-5.1", "kimi-k3", "kimi-k2.7-code", "kimi-k2.6", "deepseek-v4-pro", "deepseek-v4-flash", "mimo-v2.5", "mimo-v2.5-pro"],
        "yolo-auto"    => vec!["yolo", "yolo-small"],
        _              => vec![],
    }.into_iter().map(String::from).collect();
    m.push("Other…".into());
    m
}

/// Build and open the provider picker overlay.
pub(super) fn open_provider_picker(app: &mut App, config: &Config) {
    let gemini_ready = crate::llm_client::auth::check_gcloud_adc().is_some()
        || crate::llm_client::auth::check_google_api_key_env().is_some();
    let claude_code_ready = crate::llm_client::auth::check_claude_code().is_some();
    let codex_ready = crate::llm_client::auth::check_codex().is_some();

    // Fetch LM Studio models dynamically; fall back to hardcoded list.
    let mut lm_studio_models = crate::llm_client::fetch_openai_compatible_models(
        "http://localhost:1234/v1/chat/completions");
    if lm_studio_models.is_empty() {
        lm_studio_models = vec!["qwen3-coder-30b".into(), "devstral-small-2".into(),
            "gemma-4-e4b".into(), "qwen2.5-coder-7b-instruct".into(),
            "mistral-7b-instruct".into(), "Other…".into()];
    }

    // GoModel — built-in entry; fetches models dynamically when configured,
    // falls back to TOML-defined models, then "Other…".
    let gomodel_cfg = config.all_providers.get("gomodel");
    let gomodel_ready = gomodel_cfg.is_some();
    let gomodel_base_url = gomodel_cfg.and_then(|e| e.base_url.clone());
    let gomodel_hint: String = gomodel_cfg
        .and_then(|e| e.base_url.as_deref())
        .map(|u| u.trim_end_matches("/chat/completions").trim_end_matches('/').to_string())
        .unwrap_or_else(|| "self-hosted OpenAI-compatible gateway · needs config".to_string());
    let mut gomodel_models: Vec<String> = if let Some(cfg) = gomodel_cfg {
        let fetched = cfg.base_url.as_deref()
            .map(|url| crate::llm_client::fetch_openai_compatible_models_with_auth(
                url,
                cfg.api_key.as_deref(),
                &cfg.extra_headers,
            ))
            .unwrap_or_default();
        if !fetched.is_empty() {
            fetched
        } else {
            let mut m: Vec<String> = cfg.models.keys().cloned().collect();
            m.sort();
            if m.is_empty() {
                if let Some(model) = &cfg.model { m.push(model.clone()); }
            }
            m
        }
    } else {
        Vec::new()
    };
    gomodel_models.push("Other…".into());

    let mut entries: Vec<ProviderEntry> = vec![
        ProviderEntry { slug: "lm_studio".into(),  name: "LM Studio".into(),                  hint: "local · OpenAI-compatible".into(),             kind: ProviderKind::OpenAi,    models: lm_studio_models,                                                                                    base_url: Some("http://localhost:1234/v1/chat/completions".into()),                     needs_key: false, coming_soon: false, auth_header: None,       ready: true },
        ProviderEntry { slug: "ollama".into(),     name: "Ollama".into(),                     hint: "local · OpenAI-compatible".into(),             kind: ProviderKind::OpenAi,    models: vec!["llama3.2".into(), "llama3.1:70b".into(), "codellama".into(), "qwen2.5-coder".into(), "Other…".into()],   base_url: Some("http://localhost:11434/v1/chat/completions".into()),                      needs_key: false, coming_soon: false, auth_header: None,       ready: true },
        ProviderEntry { slug: "anthropic".into(),  name: "Anthropic".into(),                  hint: "claude-fable-5 / claude-sonnet-4-6 / claude-opus-4-8".into(), kind: ProviderKind::Anthropic, models: vec!["claude-fable-5".into(), "claude-sonnet-4-6".into(), "claude-opus-4-8".into(), "claude-haiku-4-5-20251001".into(), "Other…".into()], base_url: None,                                                                                 needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "claude_code".into(),name: "Claude Code (Pro/Max API)".into(),  hint: if claude_code_ready { "claude-fable-5 / claude-sonnet-4-6 / claude-opus-4-8 · via claude CLI".into() } else { "requires claude CLI · Pro/Max plan".into() }, kind: ProviderKind::Anthropic, models: vec!["claude-fable-5".into(), "claude-sonnet-4-6".into(), "claude-opus-4-8".into()], base_url: None,                                                                                 needs_key: false, coming_soon: !claude_code_ready, auth_header: None, ready: claude_code_ready },
        ProviderEntry { slug: "codex".into(),     name: "OpenAI Codex (ChatGPT plan)".into(), hint: if codex_ready { "gpt-5.5 / o4-mini / gpt-4.1 / o3 … · via ChatGPT subscription".into() } else { "requires codex login · ChatGPT Plus/Pro plan".into() }, kind: ProviderKind::OpenAi, models: vec!["gpt-5.5".into(), "gpt-5.4".into(), "gpt-5".into(), "gpt-4.1".into(), "o4-mini".into(), "o3".into(), "gpt-4o".into(), "Other…".into()], base_url: None,                                                                                 needs_key: false, coming_soon: false, auth_header: None, ready: codex_ready },
        ProviderEntry { slug: "openai".into(),     name: "OpenAI".into(),                     hint: "gpt-4o / gpt-4o-mini / o3".into(),             kind: ProviderKind::OpenAi,    models: vec!["gpt-4o".into(), "gpt-4o-mini".into(), "o3".into(), "o4-mini".into(), "Other…".into()],    base_url: None,                                                                                 needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "gemini".into(),     name: "Google Gemini".into(),              hint: "gemini-2.5-pro / gemini-2.0-flash".into(),     kind: ProviderKind::OpenAi,    models: vec!["gemini-2.0-flash".into(), "gemini-2.5-pro".into(), "gemini-2.5-flash".into(), "Other…".into()],     base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".into()), needs_key: true, coming_soon: false, auth_header: Some("x-goog-api-key"), ready: gemini_ready },
        ProviderEntry { slug: "deepseek".into(),   name: "DeepSeek".into(),                   hint: "deepseek-v4-pro / deepseek-v4-flash".into(),   kind: ProviderKind::OpenAi,    models: vec!["deepseek-v4-pro".into(), "deepseek-v4-flash".into(), "deepseek-chat".into(), "deepseek-reasoner".into(), "Other…".into()], base_url: Some("https://api.deepseek.com/v1/chat/completions".into()),                    needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "groq".into(),       name: "Groq".into(),                       hint: "llama-3.3-70b · fastest inference".into(),     kind: ProviderKind::OpenAi,    models: vec!["llama-3.3-70b-versatile".into(), "llama-3.1-8b-instant".into(), "mixtral-8x7b-32768".into(), "Other…".into()], base_url: Some("https://api.groq.com/openai/v1/chat/completions".into()),                   needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "mistral".into(),    name: "Mistral".into(),                    hint: "mistral-large / codestral".into(),             kind: ProviderKind::OpenAi,    models: vec!["mistral-large-latest".into(), "codestral-latest".into(), "mistral-small-latest".into(), "Other…".into()],    base_url: Some("https://api.mistral.ai/v1/chat/completions".into()),                       needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "xai".into(),        name: "xAI (Grok)".into(),                 hint: "grok-3 / grok-3-mini".into(),                  kind: ProviderKind::OpenAi,    models: vec!["grok-3".into(), "grok-3-mini".into(), "grok-2".into(), "Other…".into()],    base_url: Some("https://api.x.ai/v1/chat/completions".into()),                                needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "together".into(),   name: "Together AI".into(),                hint: "Llama / Qwen / Mistral open models".into(),    kind: ProviderKind::OpenAi,    models: vec!["meta-llama/Llama-3-70b-chat-hf".into(), "Qwen/Qwen2.5-72B-Instruct-Turbo".into(), "Other…".into()], base_url: Some("https://api.together.xyz/v1/chat/completions".into()),                      needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "perplexity".into(), name: "Perplexity".into(),                 hint: "sonar-pro · web-grounded answers".into(),      kind: ProviderKind::OpenAi,    models: vec!["sonar-pro".into(), "sonar".into(), "sonar-reasoning".into(), "Other…".into()],    base_url: Some("https://api.perplexity.ai/chat/completions".into()),                         needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "cohere".into(),     name: "Cohere".into(),                     hint: "command-a-03-2025 / command-r7b".into(),        kind: ProviderKind::OpenAi,    models: vec!["command-a-03-2025".into(), "command-r7b-12-2024".into(), "command-r-08-2024".into(), "Other…".into()], base_url: Some("https://api.cohere.ai/compatibility/v1/chat/completions".into()),            needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "openrouter".into(), name: "OpenRouter".into(),                 hint: "200+ models — Claude, GPT, Gemini, Llama…".into(),   kind: ProviderKind::OpenAi,    models: vec!["anthropic/claude-fable-5".into(), "anthropic/claude-opus-4.8".into(), "anthropic/claude-sonnet-4.6".into(), "openai/gpt-4.1".into(), "openai/gpt-4.1-mini".into(), "meta-llama/llama-4-maverick".into(), "meta-llama/llama-3.3-70b-instruct:free".into(), "google/gemini-2.5-pro".into(), "google/gemini-2.5-flash".into(), "deepseek/deepseek-r1".into(), "deepseek/deepseek-chat".into(), "qwen/qwen3-235b-a22b".into(), "mistralai/mistral-large-2512".into(), "x-ai/grok-4.20".into(), "Other…".into()], base_url: Some("https://openrouter.ai/api/v1/chat/completions".into()),          needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "kimi".into(),       name: "Kimi (Moonshot AI)".into(),         hint: "moonshot-v1 · long context up to 128k".into(),        kind: ProviderKind::OpenAi,    models: vec!["moonshot-v1-128k".into(), "moonshot-v1-32k".into(), "moonshot-v1-8k".into(), "Other…".into()], base_url: Some("https://api.moonshot.cn/v1/chat/completions".into()),             needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "zhipu".into(),      name: "Zhipu AI (GLM)".into(),             hint: "glm-4-flash free · glm-4-plus / glm-z1".into(),       kind: ProviderKind::OpenAi,    models: vec!["glm-4-flash".into(), "glm-4-air".into(), "glm-4-plus".into(), "glm-z1-flash".into(), "glm-z1-air".into(), "Other…".into()], base_url: Some("https://open.bigmodel.cn/api/paas/v4/chat/completions".into()), needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "qwen".into(),       name: "Qwen (DashScope)".into(),           hint: "qwen-turbo / qwen-max · Alibaba Cloud".into(),        kind: ProviderKind::OpenAi,    models: vec!["qwen-turbo".into(), "qwen-plus".into(), "qwen-max".into(), "qwen-long".into(), "qwen2.5-72b-instruct".into(), "qwen2.5-coder-32b-instruct".into(), "Other…".into()], base_url: Some("https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions".into()), needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "fireworks".into(),  name: "Fireworks AI".into(),               hint: "Llama / DeepSeek / Qwen · fast inference".into(),     kind: ProviderKind::OpenAi,    models: vec!["accounts/fireworks/models/llama-v3p3-70b-instruct".into(), "accounts/fireworks/models/deepseek-r1".into(), "accounts/fireworks/models/qwen3-235b-a22b".into(), "Other…".into()], base_url: Some("https://api.fireworks.ai/inference/v1/chat/completions".into()), needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "cerebras".into(),   name: "Cerebras".into(),                   hint: "gpt-oss-120b · glm-4.7 · wafer-scale inference".into(), kind: ProviderKind::OpenAi,    models: vec!["gpt-oss-120b".into(), "zai-glm-4.7".into(), "Other…".into()], base_url: Some("https://api.cerebras.ai/v1/chat/completions".into()),                needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "opencode_zen".into(), name: "OpenCode Zen (Go plan)".into(),   hint: "grok-4.5 / glm-5.2 / kimi-k3 / deepseek-v4-pro …".into(), kind: ProviderKind::OpenAi,  models: vec!["grok-4.5".into(), "glm-5.2".into(), "glm-5.1".into(), "kimi-k3".into(), "kimi-k2.7-code".into(), "kimi-k2.6".into(), "deepseek-v4-pro".into(), "deepseek-v4-flash".into(), "mimo-v2.5".into(), "mimo-v2.5-pro".into(), "Other…".into()], base_url: Some("https://opencode.ai/zen/go/v1/chat/completions".into()), needs_key: true, coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "yolo-auto".into(), name: "Yolo-Auto".into(),                  hint: "yolo / yolo-small · OpenAI-compatible · flat-rate".into(), kind: ProviderKind::OpenAi,    models: vec!["yolo".into(), "yolo-small".into(), "Other…".into()],  base_url: Some("https://yolo-auto.com/v1/chat/completions".into()),          needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
        ProviderEntry { slug: "gomodel".into(),    name: "GoModel".into(),                    hint: gomodel_hint,                                           kind: ProviderKind::OpenAi,    models: gomodel_models,                                                                        base_url: gomodel_base_url,                                                                     needs_key: !gomodel_ready, coming_soon: false, auth_header: None, ready: gomodel_ready },
        ProviderEntry { slug: "custom".into(),     name: "Custom (OpenAI-compatible)".into(), hint: "any OpenAI-compatible endpoint".into(),               kind: ProviderKind::OpenAi,    models: vec!["Other…".into()],                                                                 base_url: None,                                                                                 needs_key: false, coming_soon: false, auth_header: None,       ready: false },
    ];

    // Append user-configured providers (slugs not in the built-in list) from config.
    let known_slugs: std::collections::HashSet<&str> =
        entries.iter().map(|e| e.slug.as_str()).collect();
    let mut user_entries: Vec<ProviderEntry> = config.all_providers
        .iter()
        .filter(|(slug, _)| !known_slugs.contains(slug.as_str()))
        .map(|(slug, entry)| {
            let base = entry.base_url.as_deref()
                .unwrap_or("custom endpoint")
                .trim_end_matches("/chat/completions")
                .trim_end_matches('/');
            let active = slug == &config.provider_slug;
            let mut model_list: Vec<String> = entry.models.keys().cloned().collect();
            model_list.sort();
            if model_list.is_empty() {
                if let Some(m) = &entry.model { model_list.push(m.clone()); }
            }
            model_list.push("Other…".into());
            ProviderEntry {
                slug: slug.clone(),
                name: slug.clone(),
                hint: base.to_string(),
                kind: ProviderKind::OpenAi,
                models: model_list,
                base_url: entry.base_url.clone(),
                needs_key: false,
                coming_soon: false,
                auth_header: None,
                ready: active,
            }
        })
        .collect();
    user_entries.sort_by(|a, b| a.slug.cmp(&b.slug));
    entries.extend(user_entries);

    app.provider_picker = Some(ProviderPickerState { entries, selected: 0, is_onboarding: false });
}
