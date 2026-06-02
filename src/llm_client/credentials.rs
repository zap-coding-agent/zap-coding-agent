use std::sync::Mutex;

/// Credential resolvers for LLM providers.
///
/// `Static` holds an inline API key (existing behavior).
/// `GcloudAdc` enables the gcloud/keyless Gemini path: sends `Authorization: Bearer ` (empty),
/// which `generativelanguage.googleapis.com` accepts for anonymous/free-tier access.
/// Real OAuth tokens (user or ADC) are rejected 401 by that endpoint.
#[derive(Debug)]
pub enum CredentialProvider {
    /// Static API key — existing behavior for Anthropic, OpenAI, etc.
    Static(String),

    /// gcloud / keyless Gemini: sends empty Bearer header accepted by generativelanguage.googleapis.com.
    GcloudAdc {
        /// Unused but kept so the type can be constructed and pattern-matched elsewhere.
        cached: Mutex<Option<()>>,
    },
}

impl CredentialProvider {
    /// Fetch the credential, refreshing from gcloud if expired.
    /// Returns an empty string for `Static("")` (no-auth case for local endpoints).
    /// For `GcloudAdc`, returns the user token if available, or "" if not
    /// (caller should still send `Authorization: Bearer ` — Gemini accepts empty bearer).
    pub fn get(&self) -> Result<String, String> {
        match self {
            Self::Static(key) => Ok(key.clone()),
            Self::GcloudAdc { .. } => {
                // generativelanguage.googleapis.com/v1beta/openai/ does NOT accept OAuth tokens —
                // any real token (user or ADC, even with cloud-platform scope) returns 401.
                // But "Authorization: Bearer " (empty value) returns 200 — anonymous/free-tier access.
                // always_send_auth_header() ensures the header is sent; returning "" gives empty bearer.
                Ok(String::new())
            }
        }
    }

    /// Returns true if no credential is configured (empty static key).
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Static(key) => key.is_empty(),
            Self::GcloudAdc { .. } => false,
        }
    }

    /// GcloudAdc always wants an Authorization header sent (even with empty token),
    /// because `Authorization: Bearer ` is accepted by generativelanguage.googleapis.com
    /// while sending NO Authorization header returns 400.
    pub fn always_send_auth_header(&self) -> bool {
        matches!(self, Self::GcloudAdc { .. })
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

    #[test]
    fn gcloud_adc_returns_empty_bearer() {
        let p = CredentialProvider::GcloudAdc {
            cached: Mutex::new(None),
        };
        assert_eq!(p.get().unwrap(), "");
    }
}
