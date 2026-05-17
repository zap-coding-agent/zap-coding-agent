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
}

// ── Config file (~/.agent.toml) ───────────────────────────────────────────────

/// Serde-deserialised view of ~/.agent.toml.
/// All fields are optional so a partial file is fine.
#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    provider: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    permission_mode: Option<String>,
}

impl FileConfig {
    fn load() -> Self {
        let path = dirs::home_dir()
            .map(|h| h.join(".agent.toml"))
            .filter(|p| p.exists());

        let Some(path) = path else { return Self::default() };

        match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_else(|e| {
                eprintln!("Warning: could not parse ~/.agent.toml: {}", e);
                Self::default()
            }),
            Err(e) => {
                eprintln!("Warning: could not read ~/.agent.toml: {}", e);
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

        Ok(Self { permission_mode, api_key, model, provider, base_url, output_format: OutputFormat::Text, agent_depth: 3 })
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
        Ok(())
    }
}

