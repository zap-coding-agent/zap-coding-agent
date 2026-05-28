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

// ── B2: batch_edit for multiple replacements in the same file ─────────────────

/// B2: agent uses batch_edit (not repeated edit_file calls) when replacing
/// multiple occurrences across the same file.
#[test]
#[ignore = "requires API key — run with: cargo test --test sdk_e2e -- --ignored"]
fn b2_batch_edit_used_for_multi_replace() {
    let path = "/tmp/zap_b2_e2e.txt";
    std::fs::write(
        path,
        "PLACEHOLDER one\nPLACEHOLDER two\nPLACEHOLDER three\n",
    )
    .expect("setup: write test file");

    let prompt = format!(
        "{{\"type\":\"user\",\"text\":\"In {path} replace every occurrence of PLACEHOLDER with VALUE. Use batch_edit.\"}}\n"
    );
    let stdout = run_sdk(&[&prompt, "{\"type\":\"quit\"}\n"], Duration::from_secs(90));

    assert!(
        stdout.contains("batch_edit"),
        "expected batch_edit tool call in stdout:\n{stdout}"
    );
    let content = std::fs::read_to_string(path).expect("read result file");
    assert!(
        !content.contains("PLACEHOLDER"),
        "all PLACEHOLDERs should be replaced, got:\n{content}"
    );
    assert!(
        content.contains("VALUE"),
        "replacements should use VALUE, got:\n{content}"
    );
    let _ = std::fs::remove_file(path);
}

// ── B3: find_references before rename ────────────────────────────────────────

/// B3: agent calls find_references before editing when renaming a symbol.
#[test]
#[ignore = "requires API key — run with: cargo test --test sdk_e2e -- --ignored"]
fn b3_find_references_called_before_rename() {
    let path = "/tmp/zap_b3_e2e.rs";
    std::fs::write(
        path,
        "fn old_func() -> i32 { 42 }\nfn main() { old_func(); old_func(); }\n",
    )
    .expect("setup: write test file");

    let prompt = format!(
        "{{\"type\":\"user\",\"text\":\"Rename old_func to new_func in {path}. Find all references before editing.\"}}\n"
    );
    let stdout = run_sdk(&[&prompt, "{\"type\":\"quit\"}\n"], Duration::from_secs(120));

    // find_references must appear before any edit_file / batch_edit line.
    let ref_pos = stdout
        .find("find_references")
        .unwrap_or_else(|| panic!("find_references not called:\n{stdout}"));
    let edit_pos = stdout
        .find("edit_file")
        .or_else(|| stdout.find("batch_edit"))
        .unwrap_or_else(|| panic!("no edit tool called after find_references:\n{stdout}"));
    assert!(
        ref_pos < edit_pos,
        "find_references ({ref_pos}) must precede edit ({edit_pos})"
    );

    let content = std::fs::read_to_string(path).expect("read result file");
    assert!(
        !content.contains("old_func"),
        "old_func should be gone after rename, got:\n{content}"
    );
    assert!(
        content.contains("new_func"),
        "new_func should be present after rename, got:\n{content}"
    );
    let _ = std::fs::remove_file(path);
}

// ── B4: shell timeout param is respected ─────────────────────────────────────

/// B4: shell timeout parameter is wired through — sleep 10 with 1s timeout
/// completes well before the 60s default would have expired.
#[test]
#[ignore = "requires API key — run with: cargo test --test sdk_e2e -- --ignored"]
fn b4_shell_timeout_param_wired_through() {
    let start = std::time::Instant::now();
    let stdout = run_sdk(
        &[
            "{\"type\":\"user\",\"text\":\"Run: sleep 10 — use a 1-second timeout\"}\n",
            "{\"type\":\"quit\"}\n",
        ],
        Duration::from_secs(60),
    );
    let elapsed = start.elapsed();

    // If the old default-60s bug were present this would block for ~60s.
    // With the wired timeout the agent aborts at ~1s + LLM round-trip ≈ <20s.
    assert!(
        elapsed < Duration::from_secs(40),
        "sleep 10 with 1s timeout should abort fast, took {:?}",
        elapsed
    );
    assert!(
        stdout.contains("timed out") || stdout.contains("timeout") || stdout.contains("error"),
        "expected timeout error in output:\n{stdout}"
    );
}

// ── B5: config files read directly without code_map ──────────────────────────

/// B5: agent reads Cargo.toml directly with read_file — no code_map call first.
/// The carve-out in the code navigation strategy section allows this.
#[test]
#[ignore = "requires API key — run with: cargo test --test sdk_e2e -- --ignored"]
fn b5_config_file_read_without_code_map() {
    let stdout = run_sdk(
        &[
            "{\"type\":\"user\",\"text\":\"What is the current version in Cargo.toml? Just state the version number.\"}\n",
            "{\"type\":\"quit\"}\n",
        ],
        Duration::from_secs(60),
    );

    // read_file must be called.
    assert!(stdout.contains("read_file"), "read_file not found in stdout:\n{stdout}");

    // code_map must NOT be called before read_file (carve-out exempts manifest files).
    let read_pos = stdout.find("read_file").unwrap();
    if let Some(map_pos) = stdout.find("code_map") {
        assert!(
            map_pos > read_pos,
            "code_map ({map_pos}) must not appear before read_file ({read_pos}) for Cargo.toml"
        );
    }

    // The response must contain the correct version.
    let text = find_assistant_text(&stdout)
        .unwrap_or_else(|| panic!("no assistant response:\n{stdout}"));
    assert!(
        text.contains("0.13.57"),
        "response should contain current version 0.13.57, got: {text}"
    );
}
