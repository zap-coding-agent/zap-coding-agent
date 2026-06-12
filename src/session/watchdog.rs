/// Verify-aware progress watchdog.
///
/// Tracks failures PER VERIFICATION COMMAND: the same shell command (normalized)
/// failing N times within a turn without ever passing is the stuck signal.
/// Unlike identical-action loop detectors (OpenHands-style), this catches a
/// model trying DIFFERENT broken fixes between runs — the edits vary, but the
/// same verify command keeps failing. And unlike a global consecutive-failure
/// streak, interleaved successful diagnostics (exit-0 traces, greps, file
/// reads via shell) do NOT mask the signal: only the failing command itself
/// passing clears its counter.
///
/// Frontier-model safety: normal work is untouched. TDD red→green clears the
/// counter on green; diagnostic commands don't count; only "same command,
/// N straight failures, never passed" triggers — a state where intervention
/// is justified for any model.
///
/// Stages (per command):
/// - fails == N      → nudge: stop editing, list hypotheses, test one directly
/// - fails == 2 * N  → escalate: write a handoff summary, tools withdrawn
///
/// N defaults to 3; configurable via AGENT_VERIFY_BREAKER_N (0 disables).

#[derive(Debug, PartialEq)]
pub enum WatchdogVerdict {
    Quiet,
    Nudge,
    /// An outstanding failing verification hasn't been re-run for R rounds —
    /// the model is wandering (probing with one-off commands) instead of
    /// re-verifying. Tell it to re-run the real check now.
    StaleNudge,
    Escalate,
}

pub fn breaker_n() -> u32 {
    std::env::var("AGENT_VERIFY_BREAKER_N")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(3)
}

/// Rounds (LLM calls) an outstanding failure may go un-re-verified before the
/// staleness nudge; 2× this escalates. Evasion-proof complement to the
/// per-command fail count: a model that stops re-running the acceptance check
/// can't dodge the clock.
pub fn breaker_rounds() -> usize {
    std::env::var("AGENT_VERIFY_BREAKER_ROUNDS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(8)
}

/// A shell result counts as a failed verification when the command exited
/// non-zero (shell tool appends "[exit code: N]") or errored outright.
pub fn is_failed_verify(tool_name: &str, content: &str) -> bool {
    tool_name == "shell"
        && (content.contains("[exit code:") || content.starts_with("Error:"))
}

/// Normalize a shell command so trivial whitespace differences map to the
/// same failure counter.
pub fn normalize_cmd(cmd: &str) -> String {
    cmd.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Combined verdict for an outstanding failing command: fail-count thresholds
/// (precise) plus staleness-in-rounds (evasion-proof). `nudged` suppresses
/// repeat nudges within a turn.
pub fn assess_outstanding(
    fails: u32,
    rounds_outstanding: usize,
    n: u32,
    r: usize,
    nudged: bool,
) -> WatchdogVerdict {
    if n == 0 {
        return WatchdogVerdict::Quiet;
    }
    if fails >= 2 * n || (r > 0 && rounds_outstanding >= 2 * r) {
        return WatchdogVerdict::Escalate;
    }
    if !nudged {
        if fails >= n {
            return WatchdogVerdict::Nudge;
        }
        if r > 0 && rounds_outstanding >= r {
            return WatchdogVerdict::StaleNudge;
        }
    }
    WatchdogVerdict::Quiet
}

pub fn stale_nudge_text(cmd: &str, rounds: usize) -> String {
    format!(
        "\n\n[zap watchdog] The verification command `{cmd}` failed earlier and has NOT been \
         re-run for {rounds} rounds. One-off probe commands are not verification. Re-run \
         `{cmd}` NOW. If it still fails, stop and reconsider whether the requirements are \
         even mutually satisfiable before editing further."
    )
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
    fn command_normalization() {
        assert_eq!(normalize_cmd("node  test.js"), normalize_cmd("node test.js"));
        assert_eq!(normalize_cmd("  node test.js \n"), "node test.js");
        assert_ne!(normalize_cmd("node test.js"), normalize_cmd("node check.js"));
    }

    #[test]
    fn outstanding_combines_fails_and_staleness() {
        // fail-count path
        assert_eq!(assess_outstanding(3, 0, 3, 8, false), WatchdogVerdict::Nudge);
        assert_eq!(assess_outstanding(6, 0, 3, 8, false), WatchdogVerdict::Escalate);
        // staleness path: outstanding failure, not re-run for R rounds
        assert_eq!(assess_outstanding(1, 8, 3, 8, false), WatchdogVerdict::StaleNudge);
        assert_eq!(assess_outstanding(1, 16, 3, 8, false), WatchdogVerdict::Escalate);
        // nudged suppresses repeat nudges but never escalation
        assert_eq!(assess_outstanding(4, 0, 3, 8, true), WatchdogVerdict::Quiet);
        assert_eq!(assess_outstanding(1, 9, 3, 8, true), WatchdogVerdict::Quiet);
        assert_eq!(assess_outstanding(6, 0, 3, 8, true), WatchdogVerdict::Escalate);
        // healthy: low fails, recently re-run
        assert_eq!(assess_outstanding(1, 2, 3, 8, false), WatchdogVerdict::Quiet);
        // disabled
        assert_eq!(assess_outstanding(10, 99, 0, 8, false), WatchdogVerdict::Quiet);
    }
}
