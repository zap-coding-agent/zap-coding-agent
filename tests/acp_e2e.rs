//! ACP mode end-to-end tests.
//!
//! Spawn the compiled `zap acp` binary and speak JSON-RPC over stdio, the way
//! Zed does. Turns run against a scripted fake OpenAI-compatible server on
//! localhost, with HOME pointed at a temp dir — no API key, no network, and the
//! user's real ~/.zap and ~/.agent.toml are never touched.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ZAP: &str = env!("CARGO_BIN_EXE_zap");
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

// ── fake LLM ─────────────────────────────────────────────────────────────────

/// A canned OpenAI chat-completions reply.
fn text_reply(text: &str) -> Value {
    json!({
        "choices": [{ "message": { "role": "assistant", "content": text }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
    })
}

fn tool_reply(id: &str, name: &str, args: Value) -> Value {
    json!({
        "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [{
            "id": id, "type": "function",
            "function": { "name": name, "arguments": args.to_string() }
        }]}, "finish_reason": "tool_calls" }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
    })
}

/// Serves scripted replies in order (the last one repeats, so zap's auxiliary
/// calls — summaries, titles — also get an answer). `delay` holds every reply.
struct FakeLlm {
    url: String,
}

impl FakeLlm {
    fn start(script: Vec<Value>, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/chat/completions", listener.local_addr().unwrap());
        let script = Arc::new(Mutex::new(script));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let script = script.clone();
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.try_clone().unwrap());
                    let mut len = 0usize;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 { return; }
                        if line == "\r\n" { break; }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; len];
                    let _ = reader.read_exact(&mut body);
                    std::thread::sleep(delay);
                    let reply = {
                        let mut s = script.lock().unwrap();
                        if s.len() > 1 { s.remove(0) } else { s[0].clone() }
                    };
                    let body = reply.to_string();
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        body.len(), body
                    );
                });
            }
        });
        Self { url }
    }
}

// ── interactive client ───────────────────────────────────────────────────────

/// A minimal ACP client: writes requests, collects every inbound message, and
/// can answer the agent's own requests (permission prompts).
struct AcpClient {
    _child: KillOnDrop,
    stdin: ChildStdin,
    rx: mpsc::Receiver<Value>,
    seen: Vec<Value>,
    next_id: u64,
    _home: tempfile::TempDir,
    project: tempfile::TempDir,
}

impl AcpClient {
    /// `llm`: None = no provider configured at all.
    fn spawn(llm: Option<&FakeLlm>, extra_env: &[(&str, &str)]) -> Self {
        Self::spawn_in(tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), llm, extra_env)
    }

    fn spawn_in(home: tempfile::TempDir, project: tempfile::TempDir, llm: Option<&FakeLlm>, extra_env: &[(&str, &str)]) -> Self {
        let mut cmd = Command::new(ZAP);
        cmd.arg("acp")
            .current_dir(project.path())
            .env("HOME", home.path())
            .env("XDG_CONFIG_HOME", home.path().join(".config"))
            .env_remove("ANTHROPIC_API_KEY")
            .env_remove("OPENAI_API_KEY")
            .env_remove("GOOGLE_API_KEY")
            .env_remove("AGENT_API_KEY")
            .env_remove("AGENT_PROVIDER")
            .env_remove("AGENT_MODEL")
            .env_remove("AGENT_BASE_URL")
            .env_remove("AGENT_PERMISSION_MODE")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(llm) = llm {
            cmd.env("AGENT_PROVIDER", "openai")
                .env("AGENT_BASE_URL", &llm.url)
                .env("AGENT_API_KEY", "test-key")
                .env("AGENT_MODEL", "fake-model")
                .env("AGENT_DISABLE_STREAM", "1");
        }
        cmd.envs(extra_env.iter().copied());
        let mut child = cmd.spawn().expect("spawn zap acp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let v: Value = serde_json::from_str(&line)
                    .unwrap_or_else(|e| panic!("non-JSON on stdout ({e}): {line:?}"));
                if tx.send(v).is_err() { break; }
            }
        });
        let mut c = Self { _child: KillOnDrop(child), stdin, rx, seen: vec![], next_id: 1, _home: home, project };
        c.request("initialize", json!({ "protocolVersion": 1, "clientCapabilities": {} }));
        c
    }

    fn send(&mut self, msg: Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Send a request and return its id without waiting.
    fn start(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id
    }

    /// Wait for the response to `id`, answering permission prompts with `allow`.
    fn wait(&mut self, id: u64, allow: Option<&str>) -> Value {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = self.rx.recv_timeout(left)
                .unwrap_or_else(|_| panic!("no response to request {id}; seen: {:#?}", self.seen));
            self.seen.push(msg.clone());
            if msg["method"] == "session/request_permission" {
                let option = allow.expect("unexpected permission prompt");
                let outcome = json!({ "outcome": "selected", "optionId": option });
                self.send(json!({ "jsonrpc": "2.0", "id": msg["id"], "result": { "outcome": outcome } }));
                continue;
            }
            if msg["id"] == id && msg.get("method").is_none() {
                return msg;
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.start(method, params);
        self.wait(id, None)
    }

    fn new_session(&mut self) -> String {
        let cwd = self.project.path().to_string_lossy().to_string();
        let r = self.request("session/new", json!({ "cwd": cwd, "mcpServers": [] }));
        r["result"]["sessionId"].as_str().unwrap_or_else(|| panic!("session/new failed: {r}")).to_string()
    }

    fn prompt_params(sid: &str, text: &str) -> Value {
        json!({ "sessionId": sid, "prompt": [{ "type": "text", "text": text }] })
    }

    fn updates(&self, kind: &str) -> Vec<&Value> {
        self.seen.iter()
            .filter(|m| m["method"] == "session/update" && m["params"]["update"]["sessionUpdate"] == kind)
            .map(|m| &m["params"]["update"])
            .collect()
    }

    fn message_text(&self) -> String {
        self.updates("agent_message_chunk").iter()
            .filter_map(|u| u["content"]["text"].as_str())
            .collect()
    }

    /// Stop the agent and hand back its HOME and project dirs for a restart.
    fn finish(self) -> (tempfile::TempDir, tempfile::TempDir) {
        drop(self._child);
        (self._home, self.project)
    }
}

/// Kills the agent when a test ends — including when it panics.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
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
