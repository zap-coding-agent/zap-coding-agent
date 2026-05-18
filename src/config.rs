use anyhow::Result;
use serde::Deserialize;
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
}

// ── Config file (~/.agent.toml) ───────────────────────────────────────────────

/// Serde-deserialised view of ~/.agent.toml.
/// All fields are optional so a partial file is fine.
#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    provider:        Option<String>,
    model:           Option<String>,
    api_key:         Option<String>,
    base_url:        Option<String>,
    permission_mode: Option<String>,
    // network
    proxy:           Option<String>,
    no_proxy:        Option<String>,
    ca_bundle:       Option<String>,
    tls_skip_verify: Option<bool>,
    timeout_secs:    Option<u64>,
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

        // ── provider ──────────────────────────────────────────────────────────
        let provider_str = env::var("AGENT_PROVIDER")
            .ok()
            .or(file.provider)
            .unwrap_or_else(|| "openai".to_string()); // default: LM Studio

        let provider = match provider_str.to_lowercase().as_str() {
            "anthropic" => Provider::Anthropic,
            _           => Provider::OpenAi,
        };

        // ── api_key ───────────────────────────────────────────────────────────
        let api_key = env::var("AGENT_API_KEY").ok()
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
            .or(file.model)
            .unwrap_or(default_model);

        // ── base_url ──────────────────────────────────────────────────────────
        let default_base_url = match provider {
            Provider::Anthropic => None,
            Provider::OpenAi    => Some("http://localhost:1234".to_string()),
        };
        let base_url = env::var("AGENT_BASE_URL").ok()
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

        Ok(Self {
            permission_mode, api_key, model, provider, base_url,
            output_format: OutputFormat::Text, agent_depth: 3, is_subagent: false, spawn_depth: 0,
            proxy, no_proxy, ca_bundle, tls_skip_verify, timeout_secs,
            budget: None,
        })
    }

    /// Write current config back to ~/.agent.toml.
    pub fn save(&self) -> Result<()> {
        let path = dirs::home_dir()
            .map(|h| h.join(".agent.toml"))
            .ok_or_else(|| anyhow::anyhow!("cannot locate home directory"))?;

        let provider_str = match self.provider {
            Provider::Anthropic => "anthropic",
            Provider::OpenAi    => "openai",
        };
        let pm_str = match self.permission_mode {
            PermissionMode::Ask  => "ask",
            PermissionMode::Auto => "auto",
            PermissionMode::Deny => "deny",
        };

        let mut f = std::fs::File::create(&path)?;
        writeln!(f, "# ~/.agent.toml — managed by zap /provider")?;
        writeln!(f, "provider = {:?}", provider_str)?;
        writeln!(f, "model    = {:?}", self.model)?;
        if let Some(ref url) = self.base_url {
            writeln!(f, "base_url = {:?}", url)?;
        }
        writeln!(f, "api_key  = {:?}", self.api_key)?;
        writeln!(f)?;
        writeln!(f, "permission_mode = {:?}", pm_str)?;
        writeln!(f)?;
        writeln!(f, "# Network / corporate proxy settings")?;
        if let Some(ref p) = self.proxy {
            writeln!(f, "proxy    = {:?}", p)?;
        }
        if let Some(ref np) = self.no_proxy {
            writeln!(f, "no_proxy = {:?}", np)?;
        }
        if let Some(ref ca) = self.ca_bundle {
            writeln!(f, "ca_bundle = {:?}", ca)?;
        }
        if self.tls_skip_verify {
            writeln!(f, "tls_skip_verify = true")?;
        }
        if self.timeout_secs != 120 {
            writeln!(f, "timeout_secs = {}", self.timeout_secs)?;
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

