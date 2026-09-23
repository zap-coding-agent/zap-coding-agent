use colored::Colorize;
use inquire::{Select, Text};
use crate::config::{Config, Provider};
use super::super::Session;

/// Returns the ordered list of provider slugs shown in /provider.
/// Used by tests to assert all expected providers are registered.
pub fn provider_slugs() -> Vec<&'static str> {
    vec![
        "lm_studio", "ollama", "anthropic", "claude_code", "codex",
        "openai", "gemini", "deepseek", "groq", "mistral",
        "xai", "together", "perplexity", "cohere",
        "openrouter", "kimi", "zhipu", "qwen", "fireworks", "cerebras", "opencode_zen", "yolo-auto", "custom",
    ]
}

impl Session {
    pub fn cmd_provider(&mut self, config: &Config) {
        #[derive(Clone)]
        struct ProviderDef {
            slug:        &'static str,
            name:        &'static str,
            hint:        &'static str,
            kind:        ProviderKind,
            models:      Vec<String>,
            base_url:    Option<&'static str>,
            needs_key:   bool,
            coming_soon: bool,
            /// Custom auth header — e.g. "x-goog-api-key" for Gemini API keys.
            /// If None, defaults to "Authorization" (Bearer token).
            auth_header: Option<&'static str>,
            /// Whether credentials were auto-detected (shown as "✓ ready").
            ready:       bool,
        }
        #[derive(Clone)]
        enum ProviderKind { Anthropic, OpenAi, Codex }

        let gemini_ready = crate::llm_client::auth::check_gcloud_adc().is_some()
            || crate::llm_client::auth::check_google_api_key_env().is_some();
        let claude_code_ready = crate::llm_client::auth::check_claude_code().is_some();
        let codex_ready = crate::llm_client::auth::check_codex().is_some();
        let ollama_ready = crate::llm_client::auth::check_ollama().is_some();
        let lm_studio_ready = crate::llm_client::auth::check_lm_studio().is_some();

        // Fetch LM Studio models dynamically; fall back to hardcoded list.
        let mut lm_studio_models = crate::llm_client::fetch_openai_compatible_models(
            "http://localhost:1234/v1/chat/completions");
        if lm_studio_models.is_empty() {
            lm_studio_models = vec!["qwen3-coder-30b".into(), "devstral-small-2".into(),
                "gemma-4-e4b".into(), "qwen2.5-coder-7b-instruct".into(),
                "mistral-7b-instruct".into(), "Other…".into()];
        }

        let providers: Vec<ProviderDef> = vec![
            ProviderDef { slug: "lm_studio",  name: "LM Studio",                  hint: "local · OpenAI-compatible",                    kind: ProviderKind::OpenAi,    models: lm_studio_models,                                                                base_url: Some("http://localhost:1234/v1/chat/completions"),                                    needs_key: false, coming_soon: false, auth_header: None,       ready: lm_studio_ready },
            ProviderDef { slug: "ollama",     name: "Ollama",                     hint: "local · OpenAI-compatible",                    kind: ProviderKind::OpenAi,    models: vec!["llama3.2".into(), "llama3.1:70b".into(), "codellama".into(), "qwen2.5-coder".into(), "Other…".into()],        base_url: Some("http://localhost:11434/v1/chat/completions"),                                   needs_key: false, coming_soon: false, auth_header: None,       ready: ollama_ready },
            ProviderDef { slug: "anthropic",  name: "Anthropic",                  hint: "claude-fable-5 / claude-sonnet-4-6 / claude-opus-4-8", kind: ProviderKind::Anthropic, models: vec!["claude-fable-5".into(), "claude-sonnet-4-6".into(), "claude-opus-4-8".into(), "claude-haiku-4-5-20251001".into(), "Other…".into()], base_url: None,                                                                                needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "claude_code",name: "Claude Code (Pro/Max API)",  hint: if claude_code_ready { "auto / claude-fable-5 / claude-sonnet-4-6 · via claude CLI" } else { "requires claude CLI · Pro/Max plan" }, kind: ProviderKind::Anthropic, models: vec!["auto".into(), "claude-fable-5".into(), "claude-sonnet-4-6".into(), "claude-opus-4-8".into(), "claude-haiku-4-5-20251001".into(), "Other…".into()], base_url: None,                                                                    needs_key: false, coming_soon: !claude_code_ready, auth_header: None, ready: claude_code_ready },
            ProviderDef { slug: "codex",      name: "OpenAI Codex (ChatGPT plan)", hint: if codex_ready { "gpt-5.5 · via ChatGPT subscription" } else { "requires codex login · ChatGPT Plus/Pro plan" }, kind: ProviderKind::Codex, models: vec!["gpt-5.5".into(), "Other…".into()], base_url: None,                                                                                needs_key: false, coming_soon: false, auth_header: None, ready: codex_ready },
            ProviderDef { slug: "openai",     name: "OpenAI",                     hint: "gpt-4o / gpt-4o-mini / o3",                    kind: ProviderKind::OpenAi,    models: vec!["gpt-4o".into(), "gpt-4o-mini".into(), "o3".into(), "o4-mini".into(), "Other…".into()],             base_url: None,                                                                                needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "gemini",     name: "Google Gemini",              hint: "gemini-2.5-pro / gemini-2.0-flash",            kind: ProviderKind::OpenAi,    models: vec!["gemini-2.0-flash".into(), "gemini-2.5-pro".into(), "gemini-2.5-flash".into(), "Other…".into()],      base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"),    needs_key: true,  coming_soon: false, auth_header: Some("x-goog-api-key"), ready: gemini_ready },
            ProviderDef { slug: "deepseek",   name: "DeepSeek",                   hint: "deepseek-v4-pro / deepseek-v4-flash",         kind: ProviderKind::OpenAi,    models: vec!["deepseek-v4-pro".into(), "deepseek-v4-flash".into(), "deepseek-chat".into(), "deepseek-reasoner".into(), "Other…".into()], base_url: Some("https://api.deepseek.com/v1/chat/completions"),                           needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "groq",       name: "Groq",                       hint: "llama-3.3-70b · fastest inference",            kind: ProviderKind::OpenAi,    models: vec!["llama-3.3-70b-versatile".into(), "llama-3.1-8b-instant".into(), "mixtral-8x7b-32768".into(), "Other…".into()], base_url: Some("https://api.groq.com/openai/v1/chat/completions"),                             needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "mistral",    name: "Mistral",                    hint: "mistral-large / codestral",                    kind: ProviderKind::OpenAi,    models: vec!["mistral-large-latest".into(), "codestral-latest".into(), "mistral-small-latest".into(), "Other…".into()], base_url: Some("https://api.mistral.ai/v1/chat/completions"),                                  needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "xai",        name: "xAI (Grok)",                 hint: "grok-3 / grok-3-mini",                         kind: ProviderKind::OpenAi,    models: vec!["grok-3".into(), "grok-3-mini".into(), "grok-2".into(), "Other…".into()],    base_url: Some("https://api.x.ai/v1/chat/completions"),                                        needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "together",   name: "Together AI",                hint: "Llama / Qwen / Mistral open models",           kind: ProviderKind::OpenAi,    models: vec!["meta-llama/Llama-3-70b-chat-hf".into(), "Qwen/Qwen2.5-72B-Instruct-Turbo".into(), "Other…".into()], base_url: Some("https://api.together.xyz/v1/chat/completions"),                                needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "perplexity", name: "Perplexity",                 hint: "sonar-pro · web-grounded answers",             kind: ProviderKind::OpenAi,    models: vec!["sonar-pro".into(), "sonar".into(), "sonar-reasoning".into(), "Other…".into()],    base_url: Some("https://api.perplexity.ai/chat/completions"),                                  needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "cohere",     name: "Cohere",                     hint: "command-a-03-2025 / command-r7b",               kind: ProviderKind::OpenAi,    models: vec!["command-a-03-2025".into(), "command-r7b-12-2024".into(), "command-r-08-2024".into(), "Other…".into()], base_url: Some("https://api.cohere.ai/compatibility/v1/chat/completions"),                    needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "openrouter", name: "OpenRouter",                 hint: "200+ models — Claude, GPT, Gemini, Llama…",   kind: ProviderKind::OpenAi,    models: vec!["anthropic/claude-opus-4.8".into(), "anthropic/claude-sonnet-4.6".into(), "openai/gpt-4.1".into(), "openai/gpt-4.1-mini".into(), "meta-llama/llama-4-maverick".into(), "meta-llama/llama-3.3-70b-instruct:free".into(), "google/gemini-2.5-pro".into(), "google/gemini-2.5-flash".into(), "deepseek/deepseek-r1".into(), "deepseek/deepseek-chat".into(), "qwen/qwen3-235b-a22b".into(), "mistralai/mistral-large-2512".into(), "x-ai/grok-4.20".into(), "Other…".into()], base_url: Some("https://openrouter.ai/api/v1/chat/completions"),          needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "kimi",       name: "Kimi (Moonshot AI)",         hint: "moonshot-v1 · long context up to 128k",       kind: ProviderKind::OpenAi,    models: vec!["moonshot-v1-128k".into(), "moonshot-v1-32k".into(), "moonshot-v1-8k".into(), "Other…".into()], base_url: Some("https://api.moonshot.cn/v1/chat/completions"),             needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "zhipu",      name: "Zhipu AI (GLM)",             hint: "glm-4-flash free · glm-4-plus / glm-z1",      kind: ProviderKind::OpenAi,    models: vec!["glm-4-flash".into(), "glm-4-air".into(), "glm-4-plus".into(), "glm-z1-flash".into(), "glm-z1-air".into(), "Other…".into()], base_url: Some("https://open.bigmodel.cn/api/paas/v4/chat/completions"), needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "qwen",       name: "Qwen (DashScope)",           hint: "qwen-turbo / qwen-max · Alibaba Cloud",       kind: ProviderKind::OpenAi,    models: vec!["qwen-turbo".into(), "qwen-plus".into(), "qwen-max".into(), "qwen-long".into(), "qwen2.5-72b-instruct".into(), "qwen2.5-coder-32b-instruct".into(), "Other…".into()], base_url: Some("https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"), needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "fireworks",  name: "Fireworks AI",               hint: "Llama / DeepSeek / Qwen · fast inference",    kind: ProviderKind::OpenAi,    models: vec!["accounts/fireworks/models/llama-v3p3-70b-instruct".into(), "accounts/fireworks/models/deepseek-r1".into(), "accounts/fireworks/models/qwen3-235b-a22b".into(), "Other…".into()], base_url: Some("https://api.fireworks.ai/inference/v1/chat/completions"), needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "cerebras",   name: "Cerebras",                   hint: "gpt-oss-120b · glm-4.7 · wafer-scale inference", kind: ProviderKind::OpenAi,    models: vec!["gpt-oss-120b".into(), "zai-glm-4.7".into(), "Other…".into()], base_url: Some("https://api.cerebras.ai/v1/chat/completions"),                needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "opencode_zen", name: "OpenCode Zen (Go plan)",   hint: "grok-4.5 / glm-5.2 / kimi-k3 / deepseek-v4-pro …", kind: ProviderKind::OpenAi,    models: vec!["grok-4.5".into(), "glm-5.2".into(), "glm-5.1".into(), "kimi-k3".into(), "kimi-k2.7-code".into(), "kimi-k2.6".into(), "deepseek-v4-pro".into(), "deepseek-v4-flash".into(), "mimo-v2.5".into(), "mimo-v2.5-pro".into(), "Other…".into()], base_url: Some("https://opencode.ai/zen/go/v1/chat/completions"), needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "yolo-auto",  name: "Yolo-Auto",                  hint: "yolo / yolo-small · OpenAI-compatible · flat-rate", kind: ProviderKind::OpenAi,    models: vec!["yolo".into(), "yolo-small".into(), "Other…".into()], base_url: Some("https://yolo-auto.com/v1/chat/completions"),          needs_key: true,  coming_soon: false, auth_header: None,       ready: false },
            ProviderDef { slug: "custom",     name: "Custom (OpenAI-compatible)", hint: "any OpenAI-compatible endpoint",               kind: ProviderKind::OpenAi,    models: vec!["Other…".into()],                                                                base_url: None,                                                                                needs_key: false, coming_soon: false, auth_header: None,       ready: false },
        ];

        let mut labels: Vec<String> = providers.iter().map(|p| {
            if p.coming_soon { format!("{:<26}· {}  ◷ coming 16 Jun 2026", p.name, p.hint) }
            else             { format!("{:<26}· {}", p.name, p.hint) }
        }).collect();

        // Append user-configured providers not already in the known list.
        let known_slugs: std::collections::HashSet<&str> = provider_slugs().iter().copied().collect();
        let user_entries: Vec<(String, String)> = {
            let mut v: Vec<(String, String)> = config.all_providers
                .iter()
                .filter(|(slug, _)| !known_slugs.contains(slug.as_str()))
                .map(|(slug, entry)| {
                    let model = entry.model.as_deref().unwrap_or("?");
                    let base = entry.base_url.as_deref()
                        .unwrap_or("custom")
                        .trim_end_matches("/chat/completions")
                        .trim_end_matches('/');
                    let active = if slug == &config.provider_slug { " ✓" } else { "" };
                    (slug.clone(), format!("{:<26}· {} — {}{}", slug, model, base, active))
                })
                .collect();
            v.sort_by(|a, b| a.0.cmp(&b.0));
            v
        };
        for (_, label) in &user_entries {
            labels.push(label.clone());
        }

        let cfg = crate::ui::inquire_render_config();

        let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
        let chosen = match Select::new("Switch provider:", label_refs)
            .with_render_config(cfg)
            .with_help_message("↑↓ navigate   Enter select   Esc cancel")
            .with_page_size(14)
            .prompt_skippable()
        {
            Ok(Some(s)) => s.to_string(),
            _ => return,
        };

        let idx = labels.iter().position(|l| l == &chosen).unwrap_or(0);

        // User-configured provider selected (beyond the hardcoded list).
        if idx >= providers.len() {
            let (slug, _) = &user_entries[idx - providers.len()];
            let entry = config.all_providers.get(slug);
            let current_model = entry.and_then(|e| e.model.clone()).unwrap_or_default();

            // If the provider has a `models` map, show a picker; otherwise free text.
            let model_input = if let Some(e) = entry {
                if !e.models.is_empty() {
                    let mut model_ids: Vec<String> = e.models.keys().cloned().collect();
                    model_ids.sort();
                    model_ids.push("Other…".into());
                    match Select::new("Model:", model_ids.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                        .with_render_config(cfg)
                        .with_help_message("↑↓ navigate   Enter select   Esc cancel")
                        .with_page_size(10)
                        .prompt_skippable()
                    {
                        Ok(Some(m)) if m != "Other…" => m.to_string(),
                        Ok(Some(_)) | Ok(None) => {
                            match Text::new("Model:").with_initial_value(&current_model).with_render_config(cfg).prompt_skippable() {
                                Ok(Some(m)) if !m.trim().is_empty() => m.trim().to_string(),
                                _ => return,
                            }
                        }
                        Err(_) => return,
                    }
                } else {
                    match Text::new("Model:").with_initial_value(&current_model).with_render_config(cfg).prompt_skippable() {
                        Ok(Some(m)) if !m.trim().is_empty() => m.trim().to_string(),
                        _ => return,
                    }
                }
            } else {
                match Text::new("Model:").with_initial_value(&current_model).with_render_config(cfg).prompt_skippable() {
                    Ok(Some(m)) if !m.trim().is_empty() => m.trim().to_string(),
                    _ => return,
                }
            };
            let mut new_config = config.clone();
            new_config.provider_slug = slug.clone();
            if let Some(e) = new_config.all_providers.get_mut(slug) {
                e.model = Some(model_input.clone());
            }
            match new_config.save() {
                Ok(_)  => println!("  {} Switched to {} · {}  {}", "✓".green(), slug.cyan().bold(), model_input.cyan(), "(saved to ~/.agent.toml)".dimmed()),
                Err(e) => println!("  {} Switched to {} · {}  {} {}", "✓".green(), slug.cyan().bold(), model_input.cyan(), "warn: could not save:".yellow(), e),
            }
            return;
        }

        let def = &providers[idx];

        if def.coming_soon {
            println!();
            println!("  {} {}", "◷".truecolor(255, 210, 50), "Claude Code (Pro/Max API)".truecolor(255, 210, 50).bold());
            println!("  {}", "─".repeat(52).truecolor(60, 55, 80));
            println!("  Anthropic is adding Agent SDK credits to Pro/Max plans");
            println!("  on {} — enabling direct API access without an API key.", "16 Jun 2026".truecolor(100, 210, 255).bold());
            println!();
            println!("  {} Use {} today for Pro/Max access with an API key.",
                "tip:".truecolor(100, 95, 130), "Anthropic".truecolor(100, 210, 255));
            println!();
            return;
        }

        let base_url = if def.slug == "custom" {
            match Text::new("Full endpoint URL (e.g. http://localhost:8080/v1/chat/completions):")
                .prompt_skippable()
            {
                Ok(Some(u)) if !u.trim().is_empty() => Some(u.trim().to_string()),
                _ => { println!("  Cancelled."); return; }
            }
        } else {
            def.base_url.map(str::to_string)
        };

        let existing_entry = config.all_providers.get(def.slug);

        // Gemini with gcloud ADC: skip API key prompt entirely.
        let (api_key, credential_method) = if def.slug == "gemini" && def.ready && gemini_ready {
            // Auto-detected gcloud credentials — no key needed.
            (String::new(), Some("gcloud_adc".to_string()))
        } else if def.needs_key {
            let existing_key = existing_entry
                .and_then(|e| e.api_key.as_deref())
                .filter(|k| !k.is_empty())
                .unwrap_or("");

            let hint = if def.slug == "gemini" {
                "Keyless: run 'gcloud auth application-default login'  |  Or get a free key: aistudio.google.com/apikey"
            } else {
                "Saved to ~/.agent.toml"
            };

            let prompt = if existing_key.is_empty() {
                "API key:".to_string()
            } else {
                format!("API key (blank = keep existing {}…{}):", &existing_key[..4.min(existing_key.len())], &existing_key[existing_key.len().saturating_sub(4)..])
            };
            match Text::new(&prompt)
                .with_render_config(cfg)
                .with_help_message(hint)
                .prompt_skippable()
            {
                Ok(Some(k)) if !k.trim().is_empty() => (k.trim().to_string(), None),
                _ => (existing_key.to_string(), None),
            }
        } else {
            (String::new(), None)
        };

        let model_input = {
            match Select::new("Model:", def.models.to_vec())
                .with_render_config(cfg)
                .with_help_message("↑↓ navigate   Enter select   Esc = keep current")
                .with_page_size(10)
                .prompt_skippable()
            {
                Ok(Some(m)) => {
                    if m == "Other…" {
                        match Text::new("Enter model name:").with_render_config(cfg).prompt_skippable() {
                            Ok(Some(n)) if !n.trim().is_empty() => n.trim().to_string(),
                            _ => def.models[0].to_string(),
                        }
                    } else {
                        m.to_string()
                    }
                }
                _ => def.models[0].to_string(),
            }
        };

        let kind_str = match def.kind {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::OpenAi    => "openai",
            ProviderKind::Codex     => "codex",
        };

        let mut new_config      = config.clone();
        new_config.provider     = match def.kind {
            ProviderKind::Anthropic => Provider::Anthropic,
            ProviderKind::OpenAi    => Provider::OpenAi,
            ProviderKind::Codex     => Provider::OpenAi, // routed by slug, not provider enum
        };
        new_config.provider_slug = def.slug.to_string();
        new_config.model        = model_input.clone();
        new_config.base_url     = base_url.clone();
        new_config.api_key      = api_key.clone();

        let context_window = crate::config::default_context_window_for_provider(
            def.slug,
            Some(kind_str),
        );

        new_config.all_providers.insert(def.slug.to_string(), crate::config::ProviderEntry {
            kind:     Some(kind_str.to_string()),
            model:    Some(model_input.clone()),
            api_key:  if api_key.is_empty() { None } else { Some(api_key) },
            context_window,
            base_url: base_url.clone(),
            credential_method,
            auth_header: def.auth_header.map(|h| h.to_string()),
            extra_headers: Default::default(),
            models: Default::default(),
            tier: None,
        });

        self.client   = crate::llm_client::create_client(&new_config);
        self.model    = model_input.clone();
        self.base_url = new_config.base_url.clone();
        self.config   = new_config.clone();

        match new_config.save() {
            Ok(_)  => println!("  {} Switched to {} · {}  {}", "✓".green(), def.name.cyan().bold(), model_input.cyan(), "(saved to ~/.agent.toml)".dimmed()),
            Err(e) => println!("  {} Switched to {} · {}  {} {}", "✓".green(), def.name.cyan().bold(), model_input.cyan(), "warn: could not save:".yellow(), e),
        }
    }

    pub async fn cmd_models(&self) {
        let url = match &self.base_url {
            Some(b) => {
                let b = b.trim_end_matches('/');
                let base = b.strip_suffix("/chat/completions").unwrap_or(b);
                format!("{}/models", base.trim_end_matches('/'))
            }
            None => {
                println!("  {} /models only works with OpenAI-compatible servers.", "✗".red());
                return;
            }
        };
        // Build an authenticated request — pass the same API key and extra_headers
        // that chat completions use, so gated /v1/models endpoints don't 401.
        let entry = self.config.all_providers.get(&self.config.provider_slug);
        let client = crate::http::client();
        let mut req = client.get(&url);
        if let Some(e) = entry {
            if let Some(key) = &e.api_key {
                if !key.is_empty() && !e.extra_headers.contains_key("Authorization") {
                    req = req.header("Authorization", format!("Bearer {}", key));
                }
            }
            for (k, v) in &e.extra_headers {
                req = req.header(k, v);
            }
        }
        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => {
                        println!();
                        println!("  {}", "Available models".bold());
                        println!("  {}", "──────────────────────────────────────────".dimmed());
                        if let Some(arr) = json["data"].as_array() {
                            for m in arr {
                                let id     = m["id"].as_str().unwrap_or("?");
                                let active = if id == self.model { " ◀ active".green().to_string() } else { String::new() };
                                println!("  {} {}{}", "·".dimmed(), id.cyan(), active);
                            }
                        }
                        println!();
                        println!("  {}", "Use /model <id> to switch.".dimmed());
                        println!();
                    }
                    Err(e) => println!("  {} Failed to parse response: {}", "✗".red(), e),
                }
            }
            Ok(resp) => println!("  {} Server returned {}", "✗".red(), resp.status()),
            Err(e)   => println!("  {} Could not reach server: {}", "✗".red(), e),
        }
    }
}
