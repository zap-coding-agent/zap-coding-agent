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
    Pattern { name: "Anthropic API key",   needle: "sk-ant-" },
    Pattern { name: "OpenAI API key",      needle: "sk-proj-" },
    Pattern { name: "OpenAI API key",      needle: "sk-or-" },
    Pattern { name: "GitHub token",        needle: "ghp_" },
    Pattern { name: "GitHub token",        needle: "ghs_" },
    Pattern { name: "GitHub token",        needle: "gho_" },
    Pattern { name: "AWS access key",      needle: "akia" },        // lowercase match
    Pattern { name: "Private key block",   needle: "-----begin" },  // lowercase match
    Pattern { name: "password field",      needle: "password =" },
    Pattern { name: "password field",      needle: "password=" },
    Pattern { name: "api_key field",       needle: "api_key =" },
    Pattern { name: "api_key field",       needle: "api_key=" },
    Pattern { name: "secret field",        needle: "secret =" },
    Pattern { name: "secret field",        needle: "secret=" },
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
