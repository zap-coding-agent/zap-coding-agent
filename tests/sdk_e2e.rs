//! SDK mode end-to-end tests.
//!
//! These tests spawn the compiled `zap` binary in `--sdk --auto` mode, send
//! newline-delimited JSON prompts, and assert on the JSON output.
//!
//! All tests are marked `#[ignore]` — they require a configured API key (e.g.
//! Deepseek in ~/.agent.toml) and a compiled binary.
//!
//! Run with: `cargo test --test sdk_e2e -- --ignored`

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::time::Duration;

const ZAP: &str = env!("CARGO_BIN_EXE_zap");

/// Spawn zap in sdk+auto mode, send `lines` to stdin (each already
/// newline-terminated), wait up to `timeout` for it to finish, and return
/// stdout.
fn run_sdk(lines: &[&str], timeout: Duration) -> String {
    let mut child = Command::new(ZAP)
        .args(["--sdk", "--auto"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn zap --sdk --auto");

    {
        let stdin = child.stdin.as_mut().expect("stdin not captured");
        for line in lines {
            stdin.write_all(line.as_bytes()).expect("write to stdin failed");
        }
    }

    // Give the process up to `timeout` to finish; kill it if it hangs.
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait().expect("wait failed") {
            Some(_) => break,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                break;
            }
            None => std::thread::sleep(Duration::from_millis(200)),
        }
    }

    let output = child.wait_with_output().expect("wait_with_output failed");
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Parse stdout looking for a line that is valid JSON with `"type":"assistant"`.
fn find_assistant_text(stdout: &str) -> Option<String> {
    for line in stdout.lines() {
        let line = line.trim();
        if !line.starts_with('{') {
            continue;
        }
        // Minimal JSON parse: check for type=assistant and extract text field.
        if line.contains("\"type\":\"assistant\"") || line.contains("\"type\": \"assistant\"") {
            // Extract text value with a simple substring search.
            if let Some(start) = line.find("\"text\":") {
                let rest = &line[start + 7..].trim_start_matches(' ');
                if let Some(inner) = rest.strip_prefix('"') {
                    if let Some(end) = inner.find('"') {
                        return Some(inner[..end].to_string());
                    }
                }
            }
        }
    }
    None
}

/// Smoke test: agent responds to a greeting.
#[test]
#[ignore = "requires API key — run with: cargo test --test sdk_e2e -- --ignored"]
fn sdk_smoke_responds_to_greeting() {
    let stdout = run_sdk(
        &[
            "{\"type\":\"user\",\"text\":\"reply with exactly the word: pong\"}\n",
            "{\"type\":\"quit\"}\n",
        ],
        Duration::from_secs(60),
    );

    let text = find_assistant_text(&stdout)
        .unwrap_or_else(|| panic!("no assistant response in output:\n{stdout}"));
    assert!(
        text.to_lowercase().contains("pong"),
        "expected 'pong' in response, got: {text}"
    );
}

/// Tool execution test: shell tool is called and output appears in response.
#[test]
#[ignore = "requires API key — run with: cargo test --test sdk_e2e -- --ignored"]
fn sdk_shell_tool_returns_output() {
    let stdout = run_sdk(
        &[
            "{\"type\":\"user\",\"text\":\"run this shell command and tell me its output: echo zaptest123\"}\n",
            "{\"type\":\"quit\"}\n",
        ],
        Duration::from_secs(90),
    );

    assert!(
        stdout.contains("zaptest123"),
        "expected 'zaptest123' somewhere in stdout (tool output or assistant text):\n{stdout}"
    );
}

/// Shell timeout test: agent respects an explicit timeout parameter.
#[test]
#[ignore = "requires API key — run with: cargo test --test sdk_e2e -- --ignored"]
fn sdk_shell_timeout_respected() {
    let start = std::time::Instant::now();
    let stdout = run_sdk(
        &[
            "{\"type\":\"user\",\"text\":\"run this shell command with a 2-second timeout: sleep 10\"}\n",
            "{\"type\":\"quit\"}\n",
        ],
        Duration::from_secs(90),
    );
    let elapsed = start.elapsed();

    // The shell command (sleep 10) should be killed by the 2s timeout.
    // Total wall time should be well under 15s even accounting for LLM latency.
    assert!(
        elapsed < Duration::from_secs(30),
        "expected timeout to abort sleep 10 quickly, took {:?}",
        elapsed
    );
    // The response should mention the timeout or an error.
    assert!(
        stdout.contains("timed out") || stdout.contains("timeout") || stdout.contains("error"),
        "expected timeout mention in output:\n{stdout}"
    );
}
