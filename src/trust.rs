//! Project trust gate.
//!
//! Opening a repository must never run code that ships inside that repository
//! without the user's consent. zap loads two kinds of executable config from
//! the current working directory:
//!   - `.zap/hooks.json`  — shell commands fired on session/tool lifecycle events
//!   - `.mcp.json`        — MCP servers spawned as local processes
//!
//! Global config under `~/.zap/` is the user's own machine setup and is always
//! trusted. Project-local config is only honored when the directory is trusted,
//! so cloning and opening an untrusted repo cannot execute its hooks/servers.
//!
//! A directory is trusted when ANY of these hold:
//!   - env `ZAP_TRUST_PROJECT` is `1` / `true` / `yes`
//!   - a `.zap/trusted` marker file exists in the project
//!   - the project's canonical path is listed (one per line) in
//!     `~/.zap/trusted_dirs`

use std::path::Path;

/// Returns true if the current working directory is trusted to run its own
/// project-local hooks and MCP servers.
pub fn project_trusted() -> bool {
    env_opt_in() || marker_file_present() || listed_in_trustfile()
}

fn env_opt_in() -> bool {
    std::env::var("ZAP_TRUST_PROJECT")
        .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn marker_file_present() -> bool {
    Path::new(".zap/trusted").exists()
}

fn listed_in_trustfile() -> bool {
    let Some(home) = dirs::home_dir() else { return false };
    let Ok(contents) = std::fs::read_to_string(home.join(".zap/trusted_dirs")) else {
        return false;
    };
    let Ok(cwd) = std::env::current_dir() else { return false };
    let cwd_canon = cwd.canonicalize().unwrap_or(cwd);
    contents.lines().any(|line| {
        let t = line.trim();
        !t.is_empty()
            && Path::new(t)
                .canonicalize()
                .map(|c| c == cwd_canon)
                .unwrap_or(false)
    })
}

/// One-line hint shown when project-local config is skipped because the
/// directory is untrusted.
pub fn untrusted_hint() -> &'static str {
    "to enable it, run `touch .zap/trusted` (or set ZAP_TRUST_PROJECT=1) — \
     only do this for repositories you trust"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_enables_trust() {
        std::env::set_var("ZAP_TRUST_PROJECT", "1");
        assert!(project_trusted());
        std::env::remove_var("ZAP_TRUST_PROJECT");
    }

    #[test]
    fn env_var_false_value_does_not_trust_by_itself() {
        // Note: other signals (marker file / trustfile) could still trust this
        // dir, so we only assert the env parser rejects non-truthy values.
        std::env::set_var("ZAP_TRUST_PROJECT", "0");
        assert!(!env_opt_in());
        std::env::set_var("ZAP_TRUST_PROJECT", "nope");
        assert!(!env_opt_in());
        std::env::remove_var("ZAP_TRUST_PROJECT");
    }

    #[test]
    fn truthy_env_values_parse() {
        for v in ["1", "true", "yes", " true "] {
            std::env::set_var("ZAP_TRUST_PROJECT", v);
            assert!(env_opt_in(), "value {v:?} should opt in");
        }
        std::env::remove_var("ZAP_TRUST_PROJECT");
    }
}
