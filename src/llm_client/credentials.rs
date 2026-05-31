use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Credential resolvers for LLM providers.
///
/// `Static` holds an inline API key (existing behavior).
/// `GcloudAdc` shells out to `gcloud auth application-default print-access-token`
/// and caches the result for 50 minutes (tokens expire after 60 min by default).
#[derive(Debug)]
pub enum CredentialProvider {
    /// Static API key — existing behavior for Anthropic, OpenAI, etc.
    Static(String),

    /// gcloud Application Default Credentials.
    /// Runs `gcloud auth application-default print-access-token` on-demand.
    GcloudAdc {
        /// Cached token + timestamp. Mutex because `send()` takes `&self`.
        cached: Mutex<Option<(String, Instant)>>,
    },
}

impl CredentialProvider {
    /// Fetch the credential, refreshing from gcloud if expired.
    /// Returns an empty string for `Static("")` (no-auth case for local endpoints).
    pub fn get(&self) -> Result<String, String> {
        match self {
            Self::Static(key) => Ok(key.clone()),
            Self::GcloudAdc { cached } => {
                let mut lock = cached.lock().map_err(|e| format!("gcloud ADC lock poisoned: {e}"))?;
                if let Some((token, ts)) = lock.as_ref() {
                    if ts.elapsed() < Duration::from_secs(50 * 60) {
                        return Ok(token.clone()); // cached, still valid
                    }
                }
                // Refresh from gcloud
                let output = std::process::Command::new("gcloud")
                    .args(["auth", "application-default", "print-access-token"])
                    .output()
                    .map_err(|e| format!("gcloud failed — is gcloud CLI installed? ({e})"))?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("gcloud ADC failed: {stderr}. Run 'gcloud auth login' first."));
                }
                let token = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .to_string();
                *lock = Some((token.clone(), Instant::now()));
                Ok(token)
            }
        }
    }

    /// Returns true if no credential is configured (empty static key).
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Static(key) => key.is_empty(),
            Self::GcloudAdc { .. } => false, // gcloud always provides a token
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_returns_value() {
        let p = CredentialProvider::Static("sk-test".into());
        assert_eq!(p.get().unwrap(), "sk-test");
    }

    #[test]
    fn static_empty_is_empty() {
        let p = CredentialProvider::Static(String::new());
        assert!(p.is_empty());
        assert_eq!(p.get().unwrap(), "");
    }

    #[test]
    fn gcloud_adc_is_not_empty() {
        let p = CredentialProvider::GcloudAdc {
            cached: Mutex::new(None),
        };
        assert!(!p.is_empty());
    }

    // ── GcloudAdc caching ─────────────────────────────────────────────────

    #[test]
    fn gcloud_adc_returns_cached_token() {
        let p = CredentialProvider::GcloudAdc {
            cached: Mutex::new(Some(("cached-token".into(), Instant::now()))),
        };
        // Should return the cached token without running gcloud.
        assert_eq!(p.get().unwrap(), "cached-token");
    }

    #[test]
    fn gcloud_adc_expired_cache_refreshes() {
        // Seed with an expired timestamp so `get()` tries to run gcloud.
        let p = CredentialProvider::GcloudAdc {
            cached: Mutex::new(Some((
                "old-token".into(),
                Instant::now() - Duration::from_secs(3600),
            ))),
        };
        // If gcloud is not installed, this should return an error.
        let result = p.get();
        assert!(
            result.is_err(),
            "expired cache with no gcloud should error, got: {result:?}"
        );
        assert!(
            result.unwrap_err().contains("gcloud"),
            "error should mention gcloud"
        );
    }
}
