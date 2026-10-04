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

#[derive(Debug, Clone, PartialEq)]
pub enum SandboxMode {
    Off,
    Workdir,
    Container,
}

/// Which LLM API backend to use.
#[derive(Debug, Clone)]
pub enum Provider {
    Anthropic,
    OpenAi,
}

/// Per-model attributes under `[providers.<slug>.models."<model-id>"]`.
///
/// ```toml
/// [providers.gomodel.models."xiaomi/mimo-v2.5"]
/// name      = "mimo-v2.5"    # human-readable display name (optional)
/// reasoning = true           # reasoning/thinking model — affects token budget
/// context   = 1000000        # context window in tokens
/// output    = 128000         # max output tokens
/// ```
#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct ModelEntry {
    /// Human-readable display name shown in pickers (optional).
    pub name:      Option<String>,
    /// Whether this is a reasoning/thinking model (affects how output is interpreted).
    #[serde(default)]
    pub reasoning: bool,
    /// Context window in tokens — overrides the provider-level `context_window`.
    pub context:   Option<usize>,
    /// Maximum output tokens for this model.
    pub output:    Option<usize>,
}

/// Per-provider settings stored in the `[providers.<slug>]` TOML table.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ProviderEntry {
    /// Wire protocol: "anthropic" or "openai" (OpenAI-compatible).
    pub kind:     Option<String>,
    pub api_key:  Option<String>,
    /// Active model for this provider. When `models` is non-empty, this should
    /// be one of the keys in that map.
    pub model:    Option<String>,
    /// Provider-level context window fallback (tokens). Overridden by the
    /// per-model `context` field when the active model has an entry in `models`.
    pub context_window: Option<usize>,
    /// Full endpoint URL, e.g. "http://localhost:1234/v1/chat/completions".
    pub base_url: Option<String>,
    /// Credential resolution method: "api_key" (default) or "gcloud_adc".
    pub credential_method: Option<String>,
    /// Custom auth header name, e.g. "x-goog-api-key" for Gemini API keys.
    /// If absent, defaults to "Authorization" (Bearer token).
    pub auth_header: Option<String>,
    /// Arbitrary extra HTTP headers sent on every request.
    ///
    /// ```toml
    /// [providers.gomodel.extra_headers]
    /// X-GoModel-User-Path = "my/path"
    /// ```
    #[serde(default)]
    pub extra_headers: std::collections::HashMap<String, String>,
    /// Named model definitions with per-model attributes.
    /// Key is the model ID sent to the API (e.g. "xiaomi/mimo-v2.5").
    ///
    /// ```toml
    /// [providers.gomodel.models."xiaomi/mimo-v2.5"]
    /// name      = "mimo-v2.5"
    /// reasoning = true
    /// context   = 1000000
    /// output    = 128000
    /// ```
    #[serde(default)]
    pub models: std::collections::HashMap<String, ModelEntry>,
    /// Model tier: "slm" for small local models, "frontier" for cloud/large models.
    /// When "slm": forces core tool profile, minimal system prompt, and Ollama num_ctx.
    /// Omit to auto-detect from URL + model name (localhost + ≤13B → slm).
    pub tier: Option<String>,
}

pub const CODEX_CONTEXT_WINDOW: usize = 400_000;

mod tier;
pub use tier::{is_slm_tier, is_qwen3_8b_tier};

pub fn default_context_window_for_provider(slug: &str, kind: Option<&str>) -> Option<usize> {
    if slug == "codex" || kind.is_some_and(|k| k.eq_ignore_ascii_case("codex")) {
        Some(CODEX_CONTEXT_WINDOW)
    } else {
        None
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub permission_mode: PermissionMode,
    pub sandbox: SandboxMode,
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
    /// True when this sub-agent session was spawned by `/bg` (user-invoked,
    /// detached) rather than the model-invoked `spawn_agent` tool. Unlike plain
    /// sub-agents, background agents DO persist a `sessions` row — see
    /// `session::should_persist_session`.
    pub is_background_agent: bool,
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
    /// Maximum tokens allowed for triggered skills per turn (default 4000).
    /// When matched skills exceed this, they are ranked by priority→source→tokens
    /// and the highest-scoring skills are kept while the rest are dropped.
    /// Set in ~/.agent.toml as: skill_token_budget = 2000
    pub skill_token_budget: usize,
    /// Extra directories whose .md files are loaded as always-on project context
    /// (appended to ZAP.md / CLAUDE.md in the system prompt). Frontmatter is stripped.
    /// Set in ~/.agent.toml as: context_paths = [".kiro/steering", ".claude/context"]
    pub context_paths: Vec<String>,
    /// Extra directories the file-write tools are allowed to write to, in addition
    /// to the project root and the system temp dir. Writes outside these roots are
    /// rejected. Set in ~/.agent.toml as: allowed_paths = ["~/scratch", "/data/out"]
    pub allowed_paths: Vec<String>,
    /// Additional working directories opened alongside the primary CWD, equivalent
    /// to Claude Code's --add-dir flag. The model can read/write files in all of
    /// them. Set via `--add-dir <path>` or in ~/.agent.toml as:
    /// additional_dirs = ["../other-project", "~/shared-lib"]
    pub additional_dirs: Vec<String>,
    /// When true, send stream:false and parse a plain JSON response instead of SSE.
    /// Required for corporate proxies that mangle SSE and return empty tool_use blocks.
    pub disable_stream: bool,
    /// When true, skip the interactive `prompt_domain_scope` CLI prompt.
    /// Used by TUI mode, which shows its own in-TUI picker instead.
    pub skip_domain_prompt: bool,
    /// When true, suppress all startup println!s (skills, hooks, MCP).
    /// TUI mode shows this info in its welcome message instead.
    pub tui_mode: bool,
    /// Tool surface sent to the model: "full" (default) or "core" (file ops +
    /// shell + search only). Core cuts prompt-processing time dramatically for
    /// local small models. Env: AGENT_TOOL_PROFILE, file: tool_profile.
    pub tool_profile: String,
    /// Slug of the active provider, e.g. "anthropic", "lm_studio", "groq".
    pub provider_slug: String,
    /// All configured providers keyed by slug — preserved across /provider switches.
    pub all_providers: HashMap<String, ProviderEntry>,
    /// Tool names to exclude from every session. Set in ~/.agent.toml as:
    /// disabled_tools = ["shell", "web_fetch"]
    pub disabled_tools: Vec<String>,
    /// Skill names to exclude from every session. Set in ~/.agent.toml as:
    /// disabled_skills = ["deploy", "ship"]
    pub disabled_skills: Vec<String>,
    /// Per-task-type model overrides. Keys: "coding", "review", "explain", "search".
    /// Values: model slugs. Set in ~/.agent.toml as:
    /// [model_routes]
    /// coding = "codex/gpt-5.5"
    pub model_routes: HashMap<String, String>,
    /// Maximum number of `/bg` background agents allowed to run concurrently.
    /// Set in ~/.agent.toml as: max_background_agents = 5
    pub max_background_agents: usize,
}

// ── Config file path ──────────────────────────────────────────────────────────

/// Resolve the config file path.
///
/// Priority (first that exists wins):
///   1. `./.agent.toml`              (project-local override)
///   2. `~/.config/zap/agent.toml`   (XDG — preferred for user-global config)
///   3. `~/.agent.toml`              (legacy — kept for existing users)
///
/// If none exist, returns the XDG path (used when saving for the first time).
pub fn config_path() -> Option<std::path::PathBuf> {
    let local = std::env::current_dir().ok().map(|d| d.join(".agent.toml"));
    let xdg   = dirs::config_dir().map(|d| d.join("zap").join("agent.toml"));
    let home  = dirs::home_dir().map(|h| h.join(".agent.toml"));
    [local, xdg.clone(), home]
        .into_iter()
        .flatten()
        .find(|p| p.exists())
        .or(xdg)
}

// ── Config file (~/.agent.toml / ~/.config/zap/agent.toml) ───────────────────

/// Serde-deserialised view of the config file.
/// All fields are optional so a partial file is fine.
#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    provider:        Option<String>,
    /// Legacy top-level fields — used only when no `[providers.<slug>]` section exists.
    model:           Option<String>,
    api_key:         Option<String>,
    base_url:        Option<String>,
    permission_mode: Option<String>,
    sandbox:         Option<String>,
    /// Per-provider settings; key is slug (e.g. "anthropic", "lm_studio").
    providers:       Option<HashMap<String, ProviderEntry>>,
    // network
    proxy:           Option<String>,
    no_proxy:        Option<String>,
    ca_bundle:       Option<String>,
    tls_skip_verify: Option<bool>,
    timeout_secs:    Option<u64>,
    skill_paths:     Option<Vec<String>>,
    skill_token_budget: Option<usize>,
    context_paths:   Option<Vec<String>>,
    allowed_paths:   Option<Vec<String>>,
    additional_dirs: Option<Vec<String>>,
    disable_stream:  Option<bool>,
    tool_profile:    Option<String>,
    #[serde(default)]
    disabled_tools:  Vec<String>,
    #[serde(default)]
    disabled_skills: Vec<String>,
    #[serde(default)]
    model_routes:    HashMap<String, String>,
    max_background_agents: Option<usize>,
}

impl FileConfig {
    fn load() -> Self {
        let Some(path) = config_path().filter(|p| p.exists()) else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_else(|e| {
                crate::zap_warn!("could not parse {}: {}", path.display(), e);
                Self::default()
            }),
            Err(e) => {
                crate::zap_warn!("could not read {}: {}", path.display(), e);
                Self::default()
            }
        }
    }
}

// ── Config::load ──────────────────────────────────────────────────────────────

/// Resolve the `Provider` enum for a provider slug.
///
/// Built-in CLI-passthrough slugs (`claude_code`, `codex`) are hardcoded rather
/// than left to the `kind`/TOML fallback: `create_client()` special-cases these
/// slugs before ever consulting `config.provider`, so a missing/wrong `kind`
/// previously left `config.provider` silently mislabeled (`claude_code` resolved
/// to `OpenAi`) for every OTHER piece of code that reads it, e.g.
/// `provider_supports_vision` — it happened to work there by coincidence, not
/// because the value was actually correct.
fn resolve_provider_kind(provider_slug: &str, entry_kind: Option<&str>) -> Provider {
    match provider_slug {
        "claude_code" => Provider::Anthropic,
        "codex"       => Provider::OpenAi,
        _ => {
            // Fall back to the entry's kind, or interpret the slug name
            // (backwards compat with old provider = "anthropic").
            match entry_kind.unwrap_or(provider_slug).to_lowercase().as_str() {
                "anthropic" => Provider::Anthropic,
                _           => Provider::OpenAi,
            }
        }
    }
}

impl Config {
    /// True when the user has never set up a provider: nothing in
    /// `~/.agent.toml` and no API-key env vars. Drives first-run onboarding.
    pub fn no_provider_configured(&self) -> bool {
        self.all_providers.is_empty()
            && self.api_key.is_empty()
            && std::env::var("AGENT_API_KEY").is_err()
            && std::env::var("ANTHROPIC_API_KEY").is_err()
            && std::env::var("OPENAI_API_KEY").is_err()
            && std::env::var("GOOGLE_API_KEY").is_err()
    }

    /// Priority (highest wins): env vars → ~/.agent.toml → built-in defaults.
    pub fn load() -> Result<Self> {
        Self::load_with_provider(None)
    }

    /// Like [`Config::load`], but with `provider` as the active provider —
    /// resolving its key, model and endpoint from `~/.agent.toml`. Used to
    /// switch to an already-configured provider without an interactive picker.
    pub fn load_with_provider(provider: Option<&str>) -> Result<Self> {
        let file = FileConfig::load();

        // ── provider slug ─────────────────────────────────────────────────────
        let provider_slug = provider.map(str::to_string)
            .or_else(|| env::var("AGENT_PROVIDER").ok())
            .or(file.provider.clone())
            .unwrap_or_else(|| "lm_studio".to_string());

        // Build the full providers map from the TOML file.
        let all_providers: HashMap<String, ProviderEntry> =
            file.providers.clone().unwrap_or_default();

        // Look up the active provider entry (may be absent for legacy configs).
        let active_entry = all_providers.get(&provider_slug);

        let provider = resolve_provider_kind(&provider_slug, active_entry.and_then(|e| e.kind.as_deref()));

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
            Provider::Anthropic => "claude-opus-4-8".to_string(),
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

        // ── sandbox ────────────────────────────────────────────────────────────
        let sb_str = env::var("AGENT_SANDBOX").ok()
            .or(file.sandbox)
            .unwrap_or_else(|| "off".to_string());

        let sandbox = match sb_str.to_lowercase().as_str() {
            "off"       => SandboxMode::Off,
            "workdir"   => SandboxMode::Workdir,
            "container" => SandboxMode::Container,
            other       => anyhow::bail!("invalid sandbox '{}' — use off / workdir / container", other),
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
        let skill_token_budget = file.skill_token_budget.unwrap_or(4000);
        let context_paths  = file.context_paths.unwrap_or_default();
        let allowed_paths  = file.allowed_paths.unwrap_or_default();
        let additional_dirs = file.additional_dirs.unwrap_or_default();

        let disable_stream = env::var("AGENT_DISABLE_STREAM")
            .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
            .unwrap_or(file.disable_stream.unwrap_or(false));

        let tool_profile = env::var("AGENT_TOOL_PROFILE")
            .ok()
            .or(file.tool_profile)
            .map(|v| v.trim().to_lowercase())
            .filter(|v| v == "core" || v == "full")
            .unwrap_or_else(|| "full".to_string());

        let disabled_tools  = file.disabled_tools;
        let disabled_skills = file.disabled_skills;
        let model_routes    = file.model_routes;
        let max_background_agents = file.max_background_agents.unwrap_or(5);

        Ok(Self {
            permission_mode, sandbox, api_key, model, provider, base_url,
            output_format: OutputFormat::Text, agent_depth: 3, is_subagent: false,
            is_background_agent: false, spawn_depth: 0,
            proxy, no_proxy, ca_bundle, tls_skip_verify, timeout_secs,
            budget: None, skill_paths, skill_token_budget, context_paths, allowed_paths, additional_dirs, disable_stream, skip_domain_prompt: false, tui_mode: false,
            tool_profile, provider_slug, all_providers,
            disabled_tools, disabled_skills, model_routes, max_background_agents,
        })
    }

    /// Write config to the active config path (XDG or legacy), creating the directory if needed.
    pub fn save(&self) -> Result<()> {
        let path = config_path()
            .ok_or_else(|| anyhow::anyhow!("cannot locate config directory"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.save_to(&path)
    }

    /// Write config to an arbitrary path (used directly in tests).
    pub(crate) fn save_to(&self, path: &std::path::Path) -> Result<()> {
        let pm_str = match self.permission_mode {
            PermissionMode::Ask  => "ask",
            PermissionMode::Auto => "auto",
            PermissionMode::Deny => "deny",
        };

        let mut f = std::fs::File::create(path)?;
        writeln!(f, "# agent.toml — managed by zap /provider")?;
        writeln!(f, "provider        = {:?}", self.provider_slug)?;
        writeln!(f, "permission_mode = {:?}", pm_str)?;
        let sb_str = match self.sandbox {
            SandboxMode::Off       => "off",
            SandboxMode::Workdir   => "workdir",
            SandboxMode::Container => "container",
        };
        if sb_str != "off" {
            writeln!(f, "sandbox         = {:?}", sb_str)?;
        }
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
            if let Some(window) = entry.context_window {
                writeln!(f, "context_window = {}", window)?;
            }
            if let Some(ref url) = entry.base_url {
                writeln!(f, "base_url = {:?}", url)?;
            }
            if let Some(ref method) = entry.credential_method {
                writeln!(f, "credential_method = {:?}", method)?;
            }
            if let Some(ref hdr) = entry.auth_header {
                writeln!(f, "auth_header = {:?}", hdr)?;
            }
            if let Some(ref tier) = entry.tier {
                writeln!(f, "tier     = {:?}", tier)?;
            }
            writeln!(f)?;

            // extra_headers sub-table
            if !entry.extra_headers.is_empty() {
                writeln!(f, "[providers.{}.extra_headers]", slug)?;
                let mut hdr_keys: Vec<&String> = entry.extra_headers.keys().collect();
                hdr_keys.sort();
                for k in hdr_keys {
                    writeln!(f, "{} = {:?}", k, entry.extra_headers[k])?;
                }
                writeln!(f)?;
            }

            // models sub-tables — sorted by model ID for determinism
            if !entry.models.is_empty() {
                let mut model_ids: Vec<&String> = entry.models.keys().collect();
                model_ids.sort();
                for model_id in model_ids {
                    let m = &entry.models[model_id];
                    writeln!(f, r#"[providers.{}.models."{}"]"#, slug, model_id)?;
                    if let Some(ref name) = m.name {
                        writeln!(f, "name      = {:?}", name)?;
                    }
                    if m.reasoning {
                        writeln!(f, "reasoning = true")?;
                    }
                    if let Some(ctx) = m.context {
                        writeln!(f, "context   = {}", ctx)?;
                    }
                    if let Some(out) = m.output {
                        writeln!(f, "output    = {}", out)?;
                    }
                    writeln!(f)?;
                }
            }
        }

        // Restrict to owner-read/write only — file contains API keys.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests;
