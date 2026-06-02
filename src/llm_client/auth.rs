use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Cached result of `check_gcloud_adc()` — avoids spawning a process on every
/// provider-list render.
static GCLOUD_CACHE: Mutex<Option<(bool, Instant)>> = Mutex::new(None);

/// Returns `Some` if the user has Application Default Credentials configured
/// for Google Cloud. Checks the ADC credentials file first (instant, no subprocess),
/// then falls back to spawning gcloud from common install locations.
/// Result is cached for 60 seconds.
pub fn check_gcloud_adc() -> Option<String> {
    // Check cache first.
    {
        let lock = GCLOUD_CACHE.lock().ok()?;
        if let Some((ok, ts)) = lock.as_ref() {
            if ts.elapsed() < Duration::from_secs(60) {
                return if *ok { Some("ready".into()) } else { None };
            }
        }
    }

    // File checks are instant. Check both user credentials (gcloud auth login)
    // and ADC (gcloud auth application-default login) — either makes Gemini usable.
    let ok = user_credentials_exist() || adc_credentials_file_exists();

    if let Ok(mut lock) = GCLOUD_CACHE.lock() {
        *lock = Some((ok, Instant::now()));
    }

    if ok { Some("ready".into()) } else { None }
}

/// Checks for user credentials created by `gcloud auth login` — instant, no subprocess.
/// These tokens work with generativelanguage.googleapis.com (ADC tokens do not).
fn user_credentials_exist() -> bool {
    dirs::home_dir()
        .map(|h| h.join(".config/gcloud/credentials.db").exists())
        .unwrap_or(false)
}

/// Checks `~/.config/gcloud/application_default_credentials.json` — instant,
/// no subprocess, works regardless of whether gcloud is in PATH.
fn adc_credentials_file_exists() -> bool {
    dirs::home_dir()
        .map(|h| h.join(".config/gcloud/application_default_credentials.json").exists())
        .unwrap_or(false)
}


/// Returns `Some` if `GOOGLE_API_KEY` env var is set (fallback for API key users).
pub fn check_google_api_key_env() -> Option<String> {
    std::env::var("GOOGLE_API_KEY").ok().filter(|k| !k.is_empty())
}

/// Checks whether the `claude` binary file exists in common locations or on PATH,
/// without spawning a subprocess. Calling `claude --version` is too slow (Node.js cold start).
fn claude_binary_exists() -> bool {
    let fixed: &[&str] = &[
        "/opt/homebrew/bin/claude",
        "/usr/local/bin/claude",
    ];
    if fixed.iter().any(|p| std::path::Path::new(p).exists()) {
        return true;
    }
    // Walk PATH entries.
    std::env::var("PATH").ok()
        .as_deref()
        .unwrap_or("")
        .split(':')
        .any(|dir| std::path::Path::new(dir).join("claude").exists())
}

static CLAUDE_CODE_CACHE: Mutex<Option<(bool, Instant)>> = Mutex::new(None);

/// Returns `Some` if the Claude Code CLI is installed and authenticated.
/// Detection: `~/.claude/` config dir exists (created on first login) AND
/// the `claude` binary is reachable from PATH or common install locations.
/// Result is cached for 60 seconds.
pub fn check_claude_code() -> Option<String> {
    {
        let lock = CLAUDE_CODE_CACHE.lock().ok()?;
        if let Some((ok, ts)) = lock.as_ref() {
            if ts.elapsed() < Duration::from_secs(60) {
                return if *ok { Some("ready".into()) } else { None };
            }
        }
    }

    // Fast path: config dir exists only after `claude` has been set up.
    let config_dir_ok = dirs::home_dir()
        .map(|h| h.join(".claude").is_dir())
        .unwrap_or(false);

    // Check binary file existence instead of spawning a subprocess.
    // `claude` is a Node.js binary and `claude --version` takes 2–5 s cold — too slow for a popup.
    let ok = config_dir_ok && claude_binary_exists();

    if let Ok(mut lock) = CLAUDE_CODE_CACHE.lock() {
        *lock = Some((ok, Instant::now()));
    }

    if ok { Some("ready".into()) } else { None }
}

/// Invalidate the gcloud ADC cache so the next `check_gcloud_adc()` call
/// re-probes the filesystem and subprocess (used after launching auth).
pub fn invalidate_gcloud_cache() {
    if let Ok(mut lock) = GCLOUD_CACHE.lock() {
        *lock = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Serialize env-var tests to prevent race conditions (env vars are process-wide).
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    // ── check_google_api_key_env ──────────────────────────────────────────

    #[test]
    fn google_api_key_env_set_returns_some() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("GOOGLE_API_KEY", "test-key-123");
        assert_eq!(check_google_api_key_env(), Some("test-key-123".into()));
        std::env::remove_var("GOOGLE_API_KEY");
    }

    #[test]
    fn google_api_key_env_unset_returns_none() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::remove_var("GOOGLE_API_KEY");
        assert_eq!(check_google_api_key_env(), None);
    }

    #[test]
    fn google_api_key_env_empty_returns_none() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("GOOGLE_API_KEY", "");
        assert_eq!(check_google_api_key_env(), None);
        std::env::remove_var("GOOGLE_API_KEY");
    }

    // ── check_gcloud_adc cache ────────────────────────────────────────────

    // Serialize tests that mutate GCLOUD_CACHE (global static).
    static CACHE_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn gcloud_cache_returns_stale_without_recheck() {
        let _lock = CACHE_MUTEX.lock().unwrap();
        // Seed the cache with a "ready" result.
        {
            let mut lock = GCLOUD_CACHE.lock().unwrap();
            *lock = Some((true, std::time::Instant::now()));
        }
        // Even if gcloud is not installed, the cached "ready" should return Some.
        let result = check_gcloud_adc();
        assert!(result.is_some(), "cached ready result should return Some");
    }

    #[test]
    fn gcloud_cache_returns_stale_failure() {
        let _lock = CACHE_MUTEX.lock().unwrap();
        // Seed the cache with a "not ready" result.
        {
            let mut lock = GCLOUD_CACHE.lock().unwrap();
            *lock = Some((false, std::time::Instant::now()));
        }
        let result = check_gcloud_adc();
        assert!(result.is_none(), "cached failure should return None");
    }
}
