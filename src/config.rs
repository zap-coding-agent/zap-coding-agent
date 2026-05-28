use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::io::Write;

#[derive(Debug, Clone, PartialEq)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone)]
pub enum PermissionMode {
    Ask,
    Auto,
    Deny,
}

/// Which LLM API backend to use.
#[derive(Debug, Clone)]
pub enum Provider {
    Anthropic,
    OpenAi,
}

/// Per-provider settings stored in the `[providers.<slug>]` TOML table.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProviderEntry {
    /// Wire protocol: "anthropic" or "openai" (OpenAI-compatible).
    pub kind:     Option<String>,
    pub api_key:  Option<String>,
    pub model:    Option<String>,
    /// Full endpoint URL, e.g. "http://localhost:1234/v1/chat/completions".
    pub base_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub permission_mode: PermissionMode,
    pub api_key: String,
    pub model: String,
    pub provider: Provider,
    pub base_url: Option<String>,
    pub output_format: OutputFormat,
    /// Remaining nesting depth for sub-agents. 0 = spawning disabled.
    pub agent_depth: u8,
    /// True when this config is for a sub-agent session. Suppresses startup banners
    /// and other output that would interleave with the parent session's output.
    pub is_subagent: bool,
    /// Nesting depth of this session: 0 = top-level, 1 = first sub-agent, etc.
    /// Incremented by run_subagent; never persisted to disk.
    pub spawn_depth: u8,

    // ── Corporate / network settings ─────────────────────────────────────────
    /// Explicit proxy URL, e.g. "http://user:pass@proxy.corp.com:8080".
    /// If absent, reqwest auto-detects HTTP_PROXY / HTTPS_PROXY from the environment.
    pub proxy: Option<String>,
    /// Comma-separated hosts that bypass the proxy, e.g. "localhost,.corp.internal".
    pub no_proxy: Option<String>,
    /// Path to a PEM or DER CA certificate file for environments with TLS inspection.
    pub ca_bundle: Option<String>,
    /// Disable TLS certificate verification. Dangerous — only for broken corp proxies.
    pub tls_skip_verify: bool,
    /// HTTP request timeout in seconds (default 120).
    pub timeout_secs: u64,
    /// Optional token budget cap. When set, overrides the model's default context
    /// window for fill-% calculation. Warns at 80%, refuses at 100%.
    pub budget: Option<u32>,
    /// Extra skill directories to scan in addition to ~/.zap/skills/ and .zap/skills/.
    /// Set in ~/.agent.toml as: skill_paths = [".kiro/skills", "~/shared-skills"]
    /// Precedence (lowest → highest): bundled → ~/.zap/skills/ → skill_paths (left→right) → .zap/skills/
    pub skill_paths: Vec<String>,
    /// Extra directories whose .md files are loaded as always-on project context
    /// (appended to ZAP.md / CLAUDE.md in the system prompt). Frontmatter is stripped.
    /// Set in ~/.agent.toml as: context_paths = [".kiro/steering", ".claude/context"]
    pub context_paths: Vec<String>,
    /// When true, send stream:false and parse a plain JSON response instead of SSE.
    /// Required for corporate proxies that mangle SSE and return empty tool_use blocks.
    pub disable_stream: bool,
    /// When true, skip the interactive `prompt_domain_scope` CLI prompt.
    /// Used by TUI mode, which shows its own in-TUI picker instead.
    pub skip_domain_prompt: bool,
    /// When true, suppress all startup println!s (skills, hooks, MCP).
    /// TUI mode shows this info in its welcome message instead.
    pub tui_mode: bool,
    /// Slug of the active provider, e.g. "anthropic", "lm_studio", "groq".
    pub provider_slug: String,
    /// All configured providers keyed by slug — preserved across /provider switches.
    pub all_providers: HashMap<String, ProviderEntry>,
}

// ── Config file (~/.agent.toml) ───────────────────────────────────────────────

/// Serde-deserialised view of ~/.agent.toml.
/// All fields are optional so a partial file is fine.
#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    provider:        Option<String>,
    /// Legacy top-level fields — used only when no `[providers.<slug>]` section exists.
    model:           Option<String>,
    api_key:         Option<String>,
    base_url:        Option<String>,
    permission_mode: Option<String>,
    /// Per-provider settings; key is slug (e.g. "anthropic", "lm_studio").
    providers:       Option<HashMap<String, ProviderEntry>>,
    // network
    proxy:           Option<String>,
    no_proxy:        Option<String>,
    ca_bundle:       Option<String>,
    tls_skip_verify: Option<bool>,
    timeout_secs:    Option<u64>,
    skill_paths:     Option<Vec<String>>,
    context_paths:   Option<Vec<String>>,
    disable_stream:  Option<bool>,
}

impl FileConfig {
    fn load() -> Self {
        let path = dirs::home_dir()
            .map(|h| h.join(".agent.toml"))
            .filter(|p| p.exists());

        let Some(path) = path else { return Self::default() };

        match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_else(|e| {
                crate::zap_warn!("could not parse ~/.agent.toml: {}", e);
                Self::default()
            }),
            Err(e) => {
                crate::zap_warn!("could not read ~/.agent.toml: {}", e);
                Self::default()
            }
        }
    }
}

// ── Config::load ──────────────────────────────────────────────────────────────

impl Config {
    /// Priority (highest wins): env vars → ~/.agent.toml → built-in defaults.
    pub fn load() -> Result<Self> {
        let file = FileConfig::load();

        // ── provider slug ─────────────────────────────────────────────────────
        let provider_slug = env::var("AGENT_PROVIDER")
            .ok()
            .or(file.provider.clone())
            .unwrap_or_else(|| "lm_studio".to_string());

        // Build the full providers map from the TOML file.
        let all_providers: HashMap<String, ProviderEntry> =
            file.providers.clone().unwrap_or_default();

        // Look up the active provider entry (may be absent for legacy configs).
        let active_entry = all_providers.get(&provider_slug);

        // Determine the Provider enum from the entry's kind, or fall back to
        // interpreting the slug name (backwards compat with old provider = "anthropic").
        let provider = {
            let kind = active_entry.and_then(|e| e.kind.as_deref());
            match kind.unwrap_or(&provider_slug).to_lowercase().as_str() {
                "anthropic" => Provider::Anthropic,
                _           => Provider::OpenAi,
            }
        };

        // ── api_key ───────────────────────────────────────────────────────────
        let api_key = env::var("AGENT_API_KEY").ok()
            .or_else(|| active_entry.and_then(|e| e.api_key.clone()).filter(|k| !k.is_empty()))
            .or(file.api_key)
            .unwrap_or_else(|| match provider {
                Provider::Anthropic => env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
                Provider::OpenAi    => env::var("OPENAI_API_KEY").unwrap_or_default(),
            });

        // ── model ─────────────────────────────────────────────────────────────
        let default_model = match provider {
            Provider::Anthropic => "claude-opus-4-7".to_string(),
            Provider::OpenAi    => "gemma-4-e4b-it".to_string(),
        };
        let model = env::var("AGENT_MODEL").ok()
            .or_else(|| active_entry.and_then(|e| e.model.clone()))
            .or(file.model)
            .unwrap_or(default_model);

        // ── base_url ──────────────────────────────────────────────────────────
        let default_base_url = match provider {
            Provider::Anthropic => None,
            Provider::OpenAi    => Some("http://localhost:1234/v1/chat/completions".to_string()),
        };
        let base_url = env::var("AGENT_BASE_URL").ok()
            .or_else(|| active_entry.and_then(|e| e.base_url.clone()))
            .or(file.base_url)
            .or(default_base_url);

        // ── permission_mode ───────────────────────────────────────────────────
        let pm_str = env::var("AGENT_PERMISSION_MODE").ok()
            .or(file.permission_mode)
            .unwrap_or_else(|| "ask".to_string());

        let permission_mode = match pm_str.to_lowercase().as_str() {
            "ask"  => PermissionMode::Ask,
            "auto" => PermissionMode::Auto,
            "deny" => PermissionMode::Deny,
            other  => anyhow::bail!("invalid permission_mode '{}' — use ask / auto / deny", other),
        };

        // ── proxy ─────────────────────────────────────────────────────────────
        let proxy = env::var("AGENT_PROXY").ok().or(file.proxy);

        let no_proxy = env::var("AGENT_NO_PROXY").ok().or(file.no_proxy);

        // ── CA bundle ─────────────────────────────────────────────────────────
        // Respect the same env var names that curl and Python use.
        let ca_bundle = env::var("AGENT_CA_BUNDLE").ok()
            .or_else(|| env::var("SSL_CERT_FILE").ok())
            .or_else(|| env::var("CURL_CA_BUNDLE").ok())
            .or(file.ca_bundle);

        // ── TLS skip verify ───────────────────────────────────────────────────
        let tls_skip_verify = env::var("AGENT_TLS_SKIP_VERIFY")
            .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
            .unwrap_or(file.tls_skip_verify.unwrap_or(false));

        // ── Timeout ───────────────────────────────────────────────────────────
        let timeout_secs = env::var("AGENT_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .or(file.timeout_secs)
            .unwrap_or(120);

        let skill_paths    = file.skill_paths.unwrap_or_default();
        let context_paths  = file.context_paths.unwrap_or_default();

        let disable_stream = env::var("AGENT_DISABLE_STREAM")
            .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
            .unwrap_or(file.disable_stream.unwrap_or(false));

        Ok(Self {
            permission_mode, api_key, model, provider, base_url,
            output_format: OutputFormat::Text, agent_depth: 3, is_subagent: false, spawn_depth: 0,
            proxy, no_proxy, ca_bundle, tls_skip_verify, timeout_secs,
            budget: None, skill_paths, context_paths, disable_stream, skip_domain_prompt: false, tui_mode: false,
            provider_slug, all_providers,
        })
    }

    /// Write current config back to ~/.agent.toml.
    pub fn save(&self) -> Result<()> {
        let path = dirs::home_dir()
            .map(|h| h.join(".agent.toml"))
            .ok_or_else(|| anyhow::anyhow!("cannot locate home directory"))?;

        let pm_str = match self.permission_mode {
            PermissionMode::Ask  => "ask",
            PermissionMode::Auto => "auto",
            PermissionMode::Deny => "deny",
        };

        let mut f = std::fs::File::create(&path)?;
        writeln!(f, "# ~/.agent.toml — managed by zap /provider")?;
        writeln!(f, "provider        = {:?}", self.provider_slug)?;
        writeln!(f, "permission_mode = {:?}", pm_str)?;
        writeln!(f)?;
        writeln!(f, "# Network / corporate proxy settings")?;
        if let Some(ref p) = self.proxy {
            writeln!(f, "proxy           = {:?}", p)?;
        }
        if let Some(ref np) = self.no_proxy {
            writeln!(f, "no_proxy        = {:?}", np)?;
        }
        if let Some(ref ca) = self.ca_bundle {
            writeln!(f, "ca_bundle       = {:?}", ca)?;
        }
        if self.tls_skip_verify {
            writeln!(f, "tls_skip_verify = true")?;
        }
        if self.timeout_secs != 120 {
            writeln!(f, "timeout_secs    = {}", self.timeout_secs)?;
        }
        if self.disable_stream {
            writeln!(f, "disable_stream  = true")?;
        }
        writeln!(f)?;

        // Write one [providers.<slug>] section per configured provider.
        // Sorted by slug so the file is deterministic.
        let mut slugs: Vec<&String> = self.all_providers.keys().collect();
        slugs.sort();
        for slug in slugs {
            let entry = &self.all_providers[slug];
            writeln!(f, "[providers.{}]", slug)?;
            if let Some(ref kind) = entry.kind {
                writeln!(f, "kind     = {:?}", kind)?;
            }
            if let Some(ref model) = entry.model {
                writeln!(f, "model    = {:?}", model)?;
            }
            if let Some(ref key) = entry.api_key {
                if !key.is_empty() {
                    writeln!(f, "api_key  = {:?}", key)?;
                }
            }
            if let Some(ref url) = entry.base_url {
                writeln!(f, "base_url = {:?}", url)?;
            }
            writeln!(f)?;
        }

        // Restrict to owner-read/write only — file contains API keys.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }
}

#[cfg(test)]
impl Default for Config {
    fn default() -> Self {
        Config {
            permission_mode: PermissionMode::Auto,
            api_key: String::new(),
            model: "test-model".to_string(),
            provider: Provider::OpenAi,
            base_url: None,
            output_format: OutputFormat::Text,
            agent_depth: 0,
            is_subagent: false,
            spawn_depth: 0,
            proxy: None,
            no_proxy: None,
            ca_bundle: None,
            tls_skip_verify: false,
            timeout_secs: 120,
            budget: None,
            skill_paths: vec![],
            context_paths: vec![],
            disable_stream: false,
            skip_domain_prompt: false,
            tui_mode: false,
            provider_slug: "test".to_string(),
            all_providers: HashMap::new(),
        }
    }
}

