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
    // VCS tokens
    Pattern { name: "GitHub token",        needle: "ghp_" },
    Pattern { name: "GitHub token",        needle: "ghs_" },
    Pattern { name: "GitHub token",        needle: "gho_" },
    Pattern { name: "GitHub token",        needle: "github_pat_" },
    Pattern { name: "GitLab token",        needle: "glpat-" },
    // Cloud
    Pattern { name: "AWS access key",      needle: "akia" },
    Pattern { name: "AWS secret key",      needle: "aws_secret_access_key" },
    Pattern { name: "GCP service account", needle: "\"type\": \"service_account\"" },
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
    Pattern { name: "secret field",        needle: "secret =" },
    Pattern { name: "secret field",        needle: "secret=" },
    Pattern { name: "token field",         needle: "access_token=" },
    Pattern { name: "token field",         needle: "access_token =" },
];

/// Scan `content` and return all detected secret matches.
/// Lines are 1-indexed. Returns empty vec if nothing found.
pub fn scan(content: &str) -> Vec<SecretMatch> {
    let mut matches = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let lower = line.to_lowercase();
        for pattern in PATTERNS {
            if lower.contains(pattern.needle) {
                let preview = if line.len() > 30 {
                    format!("{}***", &line[..30.min(line.len())])
                } else {
                    line.to_string()
                };
                matches.push(SecretMatch {
                    pattern_name: pattern.name,
                    line: idx + 1,
                    preview,
                });
                break; // one match per line is enough
            }
        }
    }
    matches
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
