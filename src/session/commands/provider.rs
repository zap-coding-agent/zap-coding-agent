use colored::Colorize;
use inquire::{Select, Text};
use crate::config::{Config, Provider};
use super::super::Session;

impl Session {
    pub fn cmd_provider(&mut self, config: &Config) {
        #[derive(Clone)]
        struct ProviderDef {
            slug:        &'static str,
            name:        &'static str,
            hint:        &'static str,
            kind:        ProviderKind,
            models:      &'static [&'static str],
            base_url:    Option<&'static str>,
            needs_key:   bool,
            coming_soon: bool,
        }
        #[derive(Clone)]
        enum ProviderKind { Anthropic, OpenAi }

        let providers: Vec<ProviderDef> = vec![
            ProviderDef { slug: "lm_studio",  name: "LM Studio",                  hint: "local · OpenAI-compatible",                    kind: ProviderKind::OpenAi,    models: &["gemma-4-e4b-it", "qwen2.5-coder-7b-instruct", "mistral-7b-instruct", "Other…"],    base_url: Some("http://localhost:1234/v1/chat/completions"),                                    needs_key: false, coming_soon: false },
            ProviderDef { slug: "ollama",     name: "Ollama",                     hint: "local · OpenAI-compatible",                    kind: ProviderKind::OpenAi,    models: &["llama3.2", "llama3.1:70b", "codellama", "qwen2.5-coder", "Other…"],                 base_url: Some("http://localhost:11434/v1/chat/completions"),                                   needs_key: false, coming_soon: false },
            ProviderDef { slug: "anthropic",  name: "Anthropic",                  hint: "claude-sonnet-4-6 / claude-opus-4-7",          kind: ProviderKind::Anthropic, models: &["claude-sonnet-4-6", "claude-opus-4-7", "claude-haiku-4-5", "Other…"],               base_url: None,                                                                                needs_key: true,  coming_soon: false },
            ProviderDef { slug: "claude_code",name: "Claude Code (Pro/Max API)",  hint: "full API via subscription · after 16 Jun 2026", kind: ProviderKind::Anthropic, models: &["claude-sonnet-4-6", "claude-opus-4-7"],                                             base_url: None,                                                                                needs_key: false, coming_soon: true  },
            ProviderDef { slug: "openai",     name: "OpenAI",                     hint: "gpt-4o / gpt-4o-mini / o3",                    kind: ProviderKind::OpenAi,    models: &["gpt-4o", "gpt-4o-mini", "o3", "o4-mini", "Other…"],                                 base_url: None,                                                                                needs_key: true,  coming_soon: false },
            ProviderDef { slug: "gemini",     name: "Google Gemini",              hint: "gemini-2.5-pro / gemini-2.0-flash",            kind: ProviderKind::OpenAi,    models: &["gemini-2.0-flash", "gemini-2.5-pro", "gemini-2.5-flash", "Other…"],                 base_url: Some("https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"),    needs_key: true,  coming_soon: false },
            ProviderDef { slug: "deepseek",   name: "DeepSeek",                   hint: "deepseek-v4-pro / deepseek-v4-flash",         kind: ProviderKind::OpenAi,    models: &["deepseek-v4-pro", "deepseek-v4-flash", "deepseek-chat", "deepseek-reasoner", "Other…"], base_url: Some("https://api.deepseek.com/v1/chat/completions"),                           needs_key: true,  coming_soon: false },
            ProviderDef { slug: "groq",       name: "Groq",                       hint: "llama-3.3-70b · fastest inference",            kind: ProviderKind::OpenAi,    models: &["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "mixtral-8x7b-32768", "Other…"], base_url: Some("https://api.groq.com/openai/v1/chat/completions"),                             needs_key: true,  coming_soon: false },
            ProviderDef { slug: "mistral",    name: "Mistral",                    hint: "mistral-large / codestral",                    kind: ProviderKind::OpenAi,    models: &["mistral-large-latest", "codestral-latest", "mistral-small-latest", "Other…"],       base_url: Some("https://api.mistral.ai/v1/chat/completions"),                                  needs_key: true,  coming_soon: false },
            ProviderDef { slug: "xai",        name: "xAI (Grok)",                 hint: "grok-3 / grok-3-mini",                         kind: ProviderKind::OpenAi,    models: &["grok-3", "grok-3-mini", "grok-2", "Other…"],                                       base_url: Some("https://api.x.ai/v1/chat/completions"),                                        needs_key: true,  coming_soon: false },
            ProviderDef { slug: "together",   name: "Together AI",                hint: "Llama / Qwen / Mistral open models",           kind: ProviderKind::OpenAi,    models: &["meta-llama/Llama-3-70b-chat-hf", "Qwen/Qwen2.5-72B-Instruct-Turbo", "Other…"],    base_url: Some("https://api.together.xyz/v1/chat/completions"),                                needs_key: true,  coming_soon: false },
            ProviderDef { slug: "perplexity", name: "Perplexity",                 hint: "sonar-pro · web-grounded answers",             kind: ProviderKind::OpenAi,    models: &["sonar-pro", "sonar", "sonar-reasoning", "Other…"],                                  base_url: Some("https://api.perplexity.ai/chat/completions"),                                  needs_key: true,  coming_soon: false },
            ProviderDef { slug: "cohere",     name: "Cohere",                     hint: "command-r-plus",                               kind: ProviderKind::OpenAi,    models: &["command-r-plus", "command-r", "Other…"],                                            base_url: Some("https://api.cohere.ai/compatibility/v1/chat/completions"),                    needs_key: true,  coming_soon: false },
            ProviderDef { slug: "custom",     name: "Custom (OpenAI-compatible)", hint: "any OpenAI-compatible endpoint",               kind: ProviderKind::OpenAi,    models: &["Other…"],                                                                           base_url: None,                                                                                needs_key: false, coming_soon: false },
        ];

        let labels: Vec<String> = providers.iter().map(|p| {
            if p.coming_soon { format!("{:<26}· {}  ◷ coming 16 Jun 2026", p.name, p.hint) }
            else             { format!("{:<26}· {}", p.name, p.hint) }
        }).collect();

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

        let api_key = if def.needs_key {
            let existing_key = existing_entry
                .and_then(|e| e.api_key.as_deref())
                .filter(|k| !k.is_empty())
                .unwrap_or("");
            let prompt = if existing_key.is_empty() {
                "API key:".to_string()
            } else {
                format!("API key (blank = keep existing {}…{}):", &existing_key[..4.min(existing_key.len())], &existing_key[existing_key.len().saturating_sub(4)..])
            };
            match Text::new(&prompt)
                .with_render_config(cfg)
                .with_help_message("Saved to ~/.agent.toml")
                .prompt_skippable()
            {
                Ok(Some(k)) if !k.trim().is_empty() => k.trim().to_string(),
                _ => existing_key.to_string(),
            }
        } else {
            String::new()
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
        };

        let mut new_config      = config.clone();
        new_config.provider     = match def.kind { ProviderKind::Anthropic => Provider::Anthropic, ProviderKind::OpenAi => Provider::OpenAi };
        new_config.provider_slug = def.slug.to_string();
        new_config.model        = model_input.clone();
        new_config.base_url     = base_url.clone();
        new_config.api_key      = api_key.clone();

        new_config.all_providers.insert(def.slug.to_string(), crate::config::ProviderEntry {
            kind:     Some(kind_str.to_string()),
            model:    Some(model_input.clone()),
            api_key:  if api_key.is_empty() { None } else { Some(api_key) },
            base_url: base_url.clone(),
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
        let client = crate::http::client();
        match client.get(&url).send().await {
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
