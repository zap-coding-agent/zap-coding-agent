//! ACP mode end-to-end tests.
//!
//! Spawn the compiled `zap acp` binary, speak JSON-RPC over stdio, and assert
//! on the protocol stream. No API key needed — these exercise the handshake
//! and stdio isolation only.

use std::io::{Read as _, Write as _};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ZAP: &str = env!("CARGO_BIN_EXE_zap");

/// Spawn `zap acp`, send `lines`, close stdin, and return (stdout, stderr).
/// Kills the process if it hasn't exited after `timeout`.
fn run_acp(lines: &[&str], timeout: Duration) -> (String, String) {
    run_acp_env(lines, timeout, &[])
}

fn run_acp_env(lines: &[&str], timeout: Duration, env: &[(&str, &str)]) -> (String, String) {
    let mut child = Command::new(ZAP)
        .arg("acp")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn zap acp");

    {
        let mut stdin = child.stdin.take().expect("stdin not captured");
        for line in lines {
            stdin.write_all(line.as_bytes()).expect("write to stdin failed");
            stdin.write_all(b"\n").expect("write to stdin failed");
        }
    } // dropping stdin sends EOF

    let deadline = Instant::now() + timeout;
    while child.try_wait().expect("wait failed").is_none() {
        if Instant::now() > deadline {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.wait();

    let mut out = String::new();
    let mut err = String::new();
    child.stdout.take().unwrap().read_to_string(&mut out).unwrap();
    child.stderr.take().unwrap().read_to_string(&mut err).unwrap();
    (out, err)
}

const INIT_V1: &str = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"acp-e2e","version":"0"}}}"#;

fn json_lines(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("non-JSON on stdout ({e}): {l:?}")))
        .collect()
}

#[test]
fn initialize_handshake_returns_v1_and_agent_info() {
    let (out, err) = run_acp(&[INIT_V1], Duration::from_secs(10));
    let msgs = json_lines(&out);
    let resp = msgs
        .iter()
        .find(|m| m["id"] == 0)
        .unwrap_or_else(|| panic!("no initialize response; stdout={out:?} stderr={err:?}"));
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["result"]["protocolVersion"], 1);
    assert_eq!(resp["result"]["agentInfo"]["name"], "zap");
}

#[test]
fn client_offering_v2_is_answered_with_v1() {
    let init_v2 = INIT_V1.replace(r#""protocolVersion":1"#, r#""protocolVersion":2"#);
    let (out, _) = run_acp(&[&init_v2], Duration::from_secs(10));
    let msgs = json_lines(&out);
    let resp = msgs.iter().find(|m| m["id"] == 0).expect("no initialize response");
    assert_eq!(resp["result"]["protocolVersion"], 1);
}

#[test]
fn stdout_carries_only_json_rpc() {
    // Every line on stdout must be a JSON-RPC 2.0 message — banners, logs and
    // stray prints belong on stderr.
    let (out, _) = run_acp(&[INIT_V1], Duration::from_secs(10));
    for msg in json_lines(&out) {
        assert_eq!(msg["jsonrpc"], "2.0", "non JSON-RPC message on stdout: {msg}");
    }
}

#[test]
fn exits_when_client_closes_stdin() {
    let start = Instant::now();
    let _ = run_acp(&[INIT_V1], Duration::from_secs(10));
    assert!(start.elapsed() < Duration::from_secs(9), "zap acp did not exit on stdin EOF");
}

#[test]
fn stray_prints_and_child_output_go_to_stderr() {
    let (out, err) = run_acp_env(
        &[INIT_V1],
        Duration::from_secs(10),
        &[("ZAP_ACP_TEST_STRAY_OUTPUT", "1")],
    );
    assert!(!out.contains("stray"), "stray output leaked onto stdout: {out:?}");
    assert!(err.contains("stray println"), "println! not redirected to stderr: {err:?}");
    assert!(err.contains("stray child"), "child stdout not redirected to stderr: {err:?}");
    json_lines(&out); // and the protocol stream is still clean
}
