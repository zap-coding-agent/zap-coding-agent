use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Cached result of `check_gcloud_adc()` — avoids spawning a process on every
/// provider-list render.
static GCLOUD_CACHE: Mutex<Option<(bool, Instant)>> = Mutex::new(None);

/// Returns `Some` if `gcloud auth application-default print-access-token`
/// succeeds (i.e. the user has authenticated via `gcloud auth login`).
/// The result is cached for 60 seconds to avoid redundant shell-outs.
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

    // Run gcloud.
    let ok = std::process::Command::new("gcloud")
        .args(["auth", "application-default", "print-access-token"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    // Update cache.
    if let Ok(mut lock) = GCLOUD_CACHE.lock() {
        *lock = Some((ok, Instant::now()));
    }

    if ok { Some("ready".into()) } else { None }
}

/// Returns `Some` if `GOOGLE_API_KEY` env var is set (fallback for API key users).
pub fn check_google_api_key_env() -> Option<String> {
    std::env::var("GOOGLE_API_KEY").ok().filter(|k| !k.is_empty())
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
