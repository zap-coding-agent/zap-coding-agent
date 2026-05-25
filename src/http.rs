/// Shared HTTP client with corporate network support.
///
/// Call `http::init(config)` once at startup (Session::new), then use
/// `http::client()` everywhere instead of `reqwest::Client::new()`.
///
/// Config priority (highest wins): env var → ~/.agent.toml → built-in default.
///
/// Supported env vars:
///   HTTPS_PROXY / HTTP_PROXY   — standard proxy (reqwest auto-detects these)
///   NO_PROXY                   — comma-separated bypass list (auto-detected)
///   AGENT_PROXY                — explicit proxy URL (overrides the above)
///   AGENT_NO_PROXY             — explicit bypass list
///   AGENT_CA_BUNDLE            — path to PEM CA cert file
///   SSL_CERT_FILE              — curl-compatible alias for AGENT_CA_BUNDLE
///   CURL_CA_BUNDLE             — curl-compatible alias for AGENT_CA_BUNDLE
///   AGENT_TLS_SKIP_VERIFY=1    — disable TLS cert checks (dangerous)
///   AGENT_TIMEOUT_SECS         — request timeout in seconds (default 120)
///
/// Proxy authentication: embed credentials in the URL:
///   http://user:password@proxy.corp.com:8080
use std::sync::OnceLock;
use std::time::Duration;

use crate::config::Config;

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Initialise the global client from `config`. Must be called before the
/// first request. Safe to call multiple times — only the first call wins.
pub fn init(config: &Config) {
    CLIENT.get_or_init(|| build(config));
}

/// Return a reference to the global client.
/// Falls back to a defaults-only client if `init` was never called (e.g. in tests).
pub fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(build_defaults)
}

// ── Builder ───────────────────────────────────────────────────────────────────

pub fn build(config: &Config) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .user_agent(concat!("zap/", env!("CARGO_PKG_VERSION")))
        .tcp_keepalive(Duration::from_secs(60));

    // ── Proxy ─────────────────────────────────────────────────────────────────
    // reqwest auto-detects HTTP_PROXY / HTTPS_PROXY / NO_PROXY from the environment.
    // An explicit `proxy` in config overrides that entirely.
    if let Some(ref proxy_url) = config.proxy {
        match reqwest::Proxy::all(proxy_url) {
            Ok(mut proxy) => {
                if let Some(ref no_proxy) = config.no_proxy {
                    proxy = proxy.no_proxy(reqwest::NoProxy::from_string(no_proxy));
                }
                builder = builder.proxy(proxy);
            }
            Err(e) => crate::zap_error!("invalid proxy URL '{}': {}", proxy_url, e),
        }
    }

    // ── Custom CA bundle ──────────────────────────────────────────────────────
    // Corporate networks often terminate TLS at a proxy and re-sign with an
    // internal CA that the OS trust store may not include.
    if let Some(ref ca_path) = config.ca_bundle {
        match load_ca(ca_path) {
            Ok(cert) => { builder = builder.add_root_certificate(cert); }
            Err(e)   => crate::zap_error!("could not load CA bundle '{}': {}", ca_path, e),
        }
    }

    // ── TLS verification ──────────────────────────────────────────────────────
    if config.tls_skip_verify {
        crate::zap_warn!("TLS certificate verification is disabled (tls_skip_verify=true). Only use this on trusted networks.");
        builder = builder.danger_accept_invalid_certs(true);
    }

    builder.build().unwrap_or_else(|e| {
        crate::zap_error!("failed to build HTTP client: {}. Using defaults.", e);
        reqwest::Client::new()
    })
}

fn build_defaults() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("zap/", env!("CARGO_PKG_VERSION")))
        .tcp_keepalive(Duration::from_secs(60))
        .build()
        .unwrap_or_default()
}

fn load_ca(path: &str) -> anyhow::Result<reqwest::Certificate> {
    let pem = std::fs::read(path)?;
    // Try PEM first, then DER.
    reqwest::Certificate::from_pem(&pem)
        .or_else(|_| reqwest::Certificate::from_der(&pem))
        .map_err(|e| anyhow::anyhow!("cannot parse certificate: {}", e))
}

// ── Startup summary ───────────────────────────────────────────────────────────

/// Returns a short human-readable description of active network settings,
/// or `None` if everything is at defaults (no proxy, no custom CA, verify on).
pub fn network_summary(config: &Config) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(ref p) = config.proxy {
        // Redact credentials before displaying
        let display = redact_proxy_url(p);
        parts.push(format!("proxy={}", display));
    } else if let Some(env_proxy) = std::env::var("HTTPS_PROXY")
        .ok()
        .or_else(|| std::env::var("HTTP_PROXY").ok())
    {
        parts.push(format!("proxy={} (env)", redact_proxy_url(&env_proxy)));
    }

    if config.ca_bundle.is_some() {
        parts.push("custom-CA".to_string());
    }

    if config.tls_skip_verify {
        parts.push("TLS-verify=OFF".to_string());
    }

    if config.timeout_secs != 120 {
        parts.push(format!("timeout={}s", config.timeout_secs));
    }

    if parts.is_empty() { None } else { Some(parts.join("  ")) }
}

/// Strips `user:password@` from a proxy URL for safe display.
fn redact_proxy_url(url: &str) -> String {
    // http://user:pass@host:port → http://***@host:port
    if let Some(at) = url.rfind('@') {
        if let Some(scheme_end) = url.find("://") {
            let scheme = &url[..scheme_end + 3];
            let host   = &url[at + 1..];
            return format!("{}***@{}", scheme, host);
        }
    }
    url.to_string()
}
