/// Verify-aware progress watchdog.
///
/// Counts consecutive FAILING verification runs (shell commands that exit
/// non-zero or error) within a single user turn. Unlike identical-action loop
/// detectors (OpenHands-style), this catches a model trying DIFFERENT broken
/// fixes: the actions vary, but the verification signal never goes green.
///
/// Stages:
/// - streak == N      → nudge: stop editing, list hypotheses, test one directly
/// - streak == 2 * N  → escalate: write a handoff summary, tools withdrawn
///
/// N defaults to 3; configurable via AGENT_VERIFY_BREAKER_N (0 disables).

#[derive(Debug, PartialEq)]
pub enum WatchdogVerdict {
    Quiet,
    Nudge,
    Escalate,
}

pub fn breaker_n() -> u32 {
    std::env::var("AGENT_VERIFY_BREAKER_N")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(3)
}

/// A shell result counts as a failed verification when the command exited
/// non-zero (shell tool appends "[exit code: N]") or errored outright.
pub fn is_failed_verify(tool_name: &str, content: &str) -> bool {
    tool_name == "shell"
        && (content.contains("[exit code:") || content.starts_with("Error:"))
}

pub fn assess(streak: u32, n: u32) -> WatchdogVerdict {
    if n == 0 {
        return WatchdogVerdict::Quiet;
    }
    if streak == 2 * n {
        WatchdogVerdict::Escalate
    } else if streak == n {
        WatchdogVerdict::Nudge
    } else {
        WatchdogVerdict::Quiet
    }
}

pub fn nudge_text(streak: u32) -> String {
    format!(
        "\n\n[zap watchdog] {streak} consecutive failing verification runs. STOP editing. \
         Re-read the failing code top-to-bottom with fresh eyes before touching anything. \
         List 2-3 DISTINCT root-cause hypotheses first — include at least one about \
         conditional/validation logic (absent vs invalid fields, inverted conditions, \
         off-by-one). Then test the single most likely hypothesis directly with a minimal \
         command before editing again."
    )
}

pub fn escalate_text(streak: u32) -> String {
    format!(
        "\n\n[zap watchdog] {streak} consecutive failing verification runs — this attempt is \
         stopped. Do NOT edit any more files or run more commands. Write a concise escalation \
         summary for a stronger model or the user: (1) what was implemented and works, \
         (2) the exact failing check and its output, (3) files changed, (4) hypotheses already \
         ruled out. End your response after the summary."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_verify_detection() {
        assert!(is_failed_verify("shell", "assert failed\n[exit code: 1]"));
        assert!(is_failed_verify("shell", "Error: spawn failed"));
        assert!(!is_failed_verify("shell", "all tests passed"));
        // only shell counts — a failed edit is not a verification signal
        assert!(!is_failed_verify("edit_file", "Error: old_string not found"));
    }

    #[test]
    fn thresholds() {
        assert_eq!(assess(2, 3), WatchdogVerdict::Quiet);
        assert_eq!(assess(3, 3), WatchdogVerdict::Nudge);
        assert_eq!(assess(4, 3), WatchdogVerdict::Quiet); // nudge fires once
        assert_eq!(assess(6, 3), WatchdogVerdict::Escalate);
        assert_eq!(assess(7, 3), WatchdogVerdict::Quiet);
    }

    #[test]
    fn disabled_with_zero() {
        assert_eq!(assess(3, 0), WatchdogVerdict::Quiet);
        assert_eq!(assess(0, 0), WatchdogVerdict::Quiet);
    }
}
