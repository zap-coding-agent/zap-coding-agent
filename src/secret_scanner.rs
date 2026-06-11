/// Scan content for common secret patterns before sending to a cloud LLM.
/// Returns a list of matches so the caller can warn the user.
use std::fmt;

#[derive(Debug)]
pub struct SecretMatch {
    pub pattern_name: &'static str,
    pub line: usize,
    pub preview: String,  // first 30 chars, rest replaced with ***
}

impl fmt::Display for SecretMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {} — {}", self.line, self.pattern_name, self.preview)
    }
}

struct Pattern {
    name: &'static str,
    needle: &'static str,   // simple substring match (case-insensitive)
}

const PATTERNS: &[Pattern] = &[
    // API keys
    Pattern { name: "Anthropic API key",   needle: "sk-ant-" },
    Pattern { name: "OpenAI API key",      needle: "sk-proj-" },
    Pattern { name: "OpenAI API key (legacy)", needle: "sk-or-" },
    Pattern { name: "Stripe live key",     needle: "sk_live_" },
    Pattern { name: "Stripe test key",     needle: "sk_test_" },
    Pattern { name: "Google API key",      needle: "aiza" },          // AIza… (GCP/Gemini/Maps)
    Pattern { name: "Hugging Face token",  needle: "hf_" },
    // VCS tokens
    Pattern { name: "GitHub token",        needle: "ghp_" },
    Pattern { name: "GitHub token",        needle: "ghs_" },
    Pattern { name: "GitHub token",        needle: "gho_" },
    Pattern { name: "GitHub token",        needle: "ghu_" },
    Pattern { name: "GitHub token",        needle: "ghr_" },
    Pattern { name: "GitHub token",        needle: "github_pat_" },
    Pattern { name: "GitLab token",        needle: "glpat-" },
    Pattern { name: "npm token",           needle: "_authtoken=" },
    // Cloud
    Pattern { name: "AWS access key",      needle: "akia" },
    Pattern { name: "AWS secret key",      needle: "aws_secret_access_key" },
    Pattern { name: "GCP service account", needle: "\"type\": \"service_account\"" },
    Pattern { name: "Azure connection string", needle: "defaultendpointsprotocol=" },
    Pattern { name: "Azure storage key",   needle: "accountkey=" },
    // Chat / comms tokens
    Pattern { name: "Slack token",         needle: "xoxb-" },
    Pattern { name: "Slack token",         needle: "xoxp-" },
    Pattern { name: "Slack app token",     needle: "xapp-" },
    Pattern { name: "Slack webhook",       needle: "hooks.slack.com/services/" },
    // DB connection strings with embedded credentials
    Pattern { name: "Postgres URL",        needle: "postgres://" },
    Pattern { name: "Postgres URL",        needle: "postgresql://" },
    Pattern { name: "MySQL URL",           needle: "mysql://" },
    Pattern { name: "MongoDB URL",         needle: "mongodb://" },
    Pattern { name: "MongoDB SRV URL",     needle: "mongodb+srv://" },
    Pattern { name: "Redis URL",           needle: "redis://" },
    // Crypto / certs
    Pattern { name: "Private key block",   needle: "-----begin" },
    Pattern { name: "JWT token",           needle: "eyjh" },  // base64 '{"' prefix
    Pattern { name: "JWT token",           needle: "eyja" },
    // Generic credential fields (config files)
    Pattern { name: "password field",      needle: "password =" },
    Pattern { name: "password field",      needle: "password=" },
    Pattern { name: "password field",      needle: "passwd=" },
    Pattern { name: "api_key field",       needle: "api_key =" },
    Pattern { name: "api_key field",       needle: "api_key=" },
    Pattern { name: "api_key field",       needle: "apikey=" },
    Pattern { name: "secret field",        needle: "secret =" },
    Pattern { name: "secret field",        needle: "secret=" },
    Pattern { name: "secret field",        needle: "client_secret=" },
    Pattern { name: "token field",         needle: "access_token=" },
    Pattern { name: "token field",         needle: "access_token =" },
    Pattern { name: "bearer token",        needle: "authorization: bearer " },
];

/// Scan `content` and return all detected secret matches.
/// Lines are 1-indexed. Returns empty vec if nothing found.
pub fn scan(content: &str) -> Vec<SecretMatch> {
    let mut matches = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let lower = line.to_lowercase();
        let mut hit: Option<&'static str> = None;
        for pattern in PATTERNS {
            if lower.contains(pattern.needle) {
                hit = Some(pattern.name);
                break;
            }
        }
        // Fall back to entropy detection — catches keys with no known prefix
        // (random tokens, custom secrets). Deliberately conservative so it does
        // not redact git SHAs, hex digests, or ordinary code.
        if hit.is_none() && line_has_high_entropy_secret(line) {
            hit = Some("high-entropy token");
        }
        if let Some(name) = hit {
            let preview = if line.len() > 30 {
                format!("{}***", &line[..30.min(line.len())])
            } else {
                line.to_string()
            };
            matches.push(SecretMatch { pattern_name: name, line: idx + 1, preview });
        }
    }
    matches
}

/// True if a line contains a token that looks like a random credential:
/// long, high Shannon entropy, and mixed character classes. Tuned to avoid
/// false positives on git SHAs (all-lowercase hex), hashes, and base64 of
/// ordinary text.
fn line_has_high_entropy_secret(line: &str) -> bool {
    for token in line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=' || c == '_' || c == '-')) {
        if token.len() < 40 {
            continue;
        }
        let has_lower = token.chars().any(|c| c.is_ascii_lowercase());
        let has_upper = token.chars().any(|c| c.is_ascii_uppercase());
        let has_digit = token.chars().any(|c| c.is_ascii_digit());
        // Require all three classes — excludes all-lowercase hex (git SHAs,
        // md5/sha digests) and all-caps constants.
        if !(has_lower && has_upper && has_digit) {
            continue;
        }
        if shannon_entropy_bits_per_char(token) >= 4.0 {
            return true;
        }
    }
    false
}

fn shannon_entropy_bits_per_char(s: &str) -> f64 {
    let mut counts = [0usize; 256];
    let mut total = 0usize;
    for b in s.bytes() {
        counts[b as usize] += 1;
        total += 1;
    }
    if total == 0 {
        return 0.0;
    }
    let total_f = total as f64;
    let mut entropy = 0.0;
    for &c in counts.iter() {
        if c > 0 {
            let p = c as f64 / total_f;
            entropy -= p * p.log2();
        }
    }
    entropy
}

/// Redact secret lines from `content` in-place.
/// Returns a summary string describing what was redacted (for user notice).
pub fn redact(content: &mut String, hits: &[SecretMatch]) -> String {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mut redacted = Vec::new();
    for h in hits {
        let idx = h.line.saturating_sub(1);
        if idx < lines.len() {
            let marker = format!("[REDACTED: {}]", h.pattern_name);
            if lines[idx] != marker {
                lines[idx] = marker;
                redacted.push(h.pattern_name);
            }
        }
    }
    *content = lines.join("\n");
    redacted.sort();
    redacted.dedup();
    format!(
        "Redacted {} secret(s): {}",
        redacted.len(),
        redacted.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_prefixes() {
        assert!(!scan("ANTHROPIC_API_KEY=sk-ant-abc123").is_empty());
        assert!(!scan("token: ghp_0123456789abcdef").is_empty());
        assert!(!scan("db = postgres://user:pass@host/db").is_empty());
        assert!(!scan("AIzaSyD-EXAMPLE-key-value").is_empty());
    }

    #[test]
    fn entropy_catches_unprefixed_random_token() {
        // 48-char mixed-class random-looking token, no known prefix.
        let line = "secret_value Xy7Qp2Rk9Lm4Zb8Nv3Tc6Wd1Hf5Gj0Sa2De4Fg6Hh8K";
        assert!(!scan(line).is_empty(), "high-entropy mixed token should be caught");
    }

    #[test]
    fn entropy_ignores_git_sha_and_hex() {
        // 40-char all-lowercase hex (git SHA) — must NOT trigger.
        assert!(scan("commit 1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b").is_empty());
        // 64-char lowercase hex digest — must NOT trigger.
        assert!(scan("sha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef").is_empty());
    }

    #[test]
    fn entropy_ignores_ordinary_prose_and_paths() {
        assert!(scan("The quick brown fox jumps over the lazy dog repeatedly today").is_empty());
        assert!(scan("/Users/someone/personal-repos/ideas/src/secret_scanner.rs").is_empty());
    }

    #[test]
    fn redact_replaces_the_line() {
        let mut c = "before\nAPI_KEY=sk-ant-supersecretvalue\nafter".to_string();
        let hits = scan(&c);
        let summary = redact(&mut c, &hits);
        assert!(c.contains("[REDACTED:"));
        assert!(!c.contains("supersecretvalue"));
        assert!(c.contains("before") && c.contains("after"));
        assert!(summary.starts_with("Redacted"));
    }
}
