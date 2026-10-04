//! ACP mode end-to-end tests: handshake, stdio isolation and session turns.
//!
//! Spawn the compiled `zap acp` binary and speak JSON-RPC over stdio, the way
//! Zed does. Turns run against a scripted fake OpenAI-compatible server on
//! localhost, with HOME pointed at a temp dir — no API key, no network, and the
//! user's real ~/.zap and ~/.agent.toml are never touched.
//! Slash commands are covered in `acp_commands_e2e.rs`.

mod acp_support;

use acp_support::{text_reply, tool_reply, AcpClient, FakeLlm, ZAP};
use serde_json::{json, Value};
use std::io::{Read as _, Write as _};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const INIT_V1: &str = r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"acp-e2e","version":"0"}}}"#;

// ── one-shot runs (handshake / stdio isolation) ──────────────────────────────

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

fn json_lines(stdout: &str) -> Vec<Value> {
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
    assert_eq!(resp["result"]["agentCapabilities"]["loadSession"], true);
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

// ── session scenarios ────────────────────────────────────────────────────────

#[test]
fn prompt_streams_reply_and_ends_turn() {
    let llm = FakeLlm::start(vec![text_reply("Hello from zap")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let sid = c.new_session();
    let r = c.request("session/prompt", AcpClient::prompt_params(&sid, "say hi"));
    assert_eq!(r["result"]["stopReason"], "end_turn", "{r}");
    assert!(c.message_text().contains("Hello from zap"), "streamed text: {:?}", c.message_text());
}

#[test]
fn new_session_advertises_permission_modes() {
    let llm = FakeLlm::start(vec![text_reply("ok")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let cwd = c.project.path().to_string_lossy().to_string();
    let r = c.request("session/new", json!({ "cwd": cwd, "mcpServers": [] }));
    let modes = &r["result"]["modes"];
    let ids: Vec<&str> = modes["availableModes"].as_array().unwrap().iter()
        .filter_map(|m| m["id"].as_str()).collect();
    assert_eq!(ids, ["ask", "auto", "read-only"]);

    let sid = r["result"]["sessionId"].as_str().unwrap().to_string();
    let r = c.request("session/set_mode", json!({ "sessionId": sid, "modeId": "read-only" }));
    assert!(r.get("error").is_none(), "set_mode failed: {r}");
    let r = c.request("session/set_mode", json!({ "sessionId": sid, "modeId": "bogus" }));
    assert!(r.get("error").is_some(), "bogus mode accepted: {r}");
}

#[test]
fn tool_call_asks_permission_streams_diff_and_writes_file() {
    let llm = FakeLlm::start(
        vec![
            tool_reply("call_1", "write_file", json!({ "path": "hello.txt", "content": "hi there\n" })),
            text_reply("Wrote hello.txt"),
        ],
        Duration::ZERO,
    );
    let mut c = AcpClient::spawn(Some(&llm), &[("AGENT_PERMISSION_MODE", "ask")]);
    let sid = c.new_session();
    let id = c.start("session/prompt", AcpClient::prompt_params(&sid, "create hello.txt"));
    let r = c.wait(id, Some("allow_once"));
    assert_eq!(r["result"]["stopReason"], "end_turn", "{r}");

    let perm = c.seen.iter().find(|m| m["method"] == "session/request_permission")
        .expect("no permission prompt");
    assert_eq!(perm["params"]["toolCall"]["toolCallId"], "call_1");
    assert_eq!(perm["params"]["toolCall"]["kind"], "edit");

    // Announced as pending before the prompt, then run, then completed with a diff.
    let calls = c.updates("tool_call");
    assert_eq!(calls[0]["toolCallId"], "call_1");
    // `pending` is the protocol default, so the wire may omit it.
    assert!(calls[0]["status"].is_null() || calls[0]["status"] == "pending", "{}", calls[0]);
    let done = c.updates("tool_call_update").into_iter()
        .find(|u| u["status"] == "completed").expect("no completed tool_call_update");
    assert_eq!(done["content"][0]["type"], "diff");
    assert!(done["content"][0]["path"].as_str().unwrap().ends_with("/hello.txt"));
    assert_eq!(done["content"][0]["newText"], "hi there\n");

    let written = std::fs::read_to_string(c.project.path().join("hello.txt")).unwrap();
    assert_eq!(written, "hi there\n");
}

#[test]
fn rejected_permission_leaves_file_untouched() {
    let llm = FakeLlm::start(
        vec![
            tool_reply("call_1", "write_file", json!({ "path": "nope.txt", "content": "x" })),
            text_reply("ok, not writing"),
        ],
        Duration::ZERO,
    );
    let mut c = AcpClient::spawn(Some(&llm), &[("AGENT_PERMISSION_MODE", "ask")]);
    let sid = c.new_session();
    let id = c.start("session/prompt", AcpClient::prompt_params(&sid, "create nope.txt"));
    let r = c.wait(id, Some("reject_once"));
    assert_eq!(r["result"]["stopReason"], "end_turn", "{r}");
    assert!(!c.project.path().join("nope.txt").exists());
}

#[test]
fn cancel_stops_a_running_turn() {
    // The LLM takes 20s to answer; cancel must not wait for it.
    let llm = FakeLlm::start(vec![text_reply("too late")], Duration::from_secs(20));
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let sid = c.new_session();
    let id = c.start("session/prompt", AcpClient::prompt_params(&sid, "slow"));
    std::thread::sleep(Duration::from_millis(500));
    let t0 = Instant::now();
    c.notify("session/cancel", json!({ "sessionId": sid }));
    let r = c.wait(id, None);
    assert_eq!(r["result"]["stopReason"], "cancelled", "{r}");
    assert!(t0.elapsed() < Duration::from_secs(5), "cancel took {:?}", t0.elapsed());
}

#[test]
fn no_provider_configured_returns_auth_required() {
    let mut c = AcpClient::spawn(None, &[]);
    let cwd = c.project.path().to_string_lossy().to_string();
    let r = c.request("session/new", json!({ "cwd": cwd, "mcpServers": [] }));
    assert_eq!(r["error"]["code"], -32000, "expected auth_required: {r}");
}

#[test]
fn load_session_replays_history() {
    let llm = FakeLlm::start(vec![text_reply("Remember me")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let sid = c.new_session();
    c.request("session/prompt", AcpClient::prompt_params(&sid, "first message"));
    let (home, project) = c.finish();

    // A fresh process (as after an editor restart) with the same HOME.
    let mut c = AcpClient::spawn_in(home, project, Some(&llm), &[]);
    let cwd = c.project.path().to_string_lossy().to_string();
    let r = c.request("session/load", json!({ "sessionId": sid, "cwd": cwd, "mcpServers": [] }));
    assert!(r.get("error").is_none(), "session/load failed: {r}");
    let user: String = c.updates("user_message_chunk").iter()
        .filter_map(|u| u["content"]["text"].as_str()).collect();
    assert!(user.contains("first message"), "replayed user text: {user:?}");
    assert!(c.message_text().contains("Remember me"));
}

#[test]
fn new_thread_starts_with_an_empty_conversation() {
    // In the TUI a new session auto-resumes the previous conversation; an
    // editor thread must not inherit another thread's hidden history.
    let llm = FakeLlm::start(vec![text_reply("ok")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let first = c.new_session();
    c.say(&first, "a question from the first thread");
    let (home, project) = c.finish();

    let mut c = AcpClient::spawn_in(home, project, Some(&llm), &[]);
    let second = c.new_session();
    let out = c.say(&second, "/history");
    assert!(out.contains("0 messages in history"), "new thread inherited history: {out:?}");
}
