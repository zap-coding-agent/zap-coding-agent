//! Shared harness for the ACP end-to-end tests: a scripted fake
//! OpenAI-compatible server and a minimal interactive ACP client that drives
//! the real `zap acp` binary in a temp HOME and project directory.
#![allow(dead_code)] // each test binary uses a different subset

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const ZAP: &str = env!("CARGO_BIN_EXE_zap");

// ── fake LLM ─────────────────────────────────────────────────────────────────

/// A canned OpenAI chat-completions reply.
pub fn text_reply(text: &str) -> Value {
    json!({
        "choices": [{ "message": { "role": "assistant", "content": text }, "finish_reason": "stop" }],
        "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
    })
}

pub fn tool_reply(id: &str, name: &str, args: Value) -> Value {
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
pub struct FakeLlm {
    pub url: String,
}

impl FakeLlm {
    pub fn start(script: Vec<Value>, delay: Duration) -> Self {
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
pub struct AcpClient {
    _child: KillOnDrop,
    stdin: ChildStdin,
    rx: mpsc::Receiver<Value>,
    pub seen: Vec<Value>,
    next_id: u64,
    _home: tempfile::TempDir,
    pub project: tempfile::TempDir,
}

impl AcpClient {
    /// `llm`: None = no provider configured at all.
    pub fn spawn(llm: Option<&FakeLlm>, extra_env: &[(&str, &str)]) -> Self {
        Self::spawn_in(tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), llm, extra_env)
    }

    pub fn spawn_in(home: tempfile::TempDir, project: tempfile::TempDir, llm: Option<&FakeLlm>, extra_env: &[(&str, &str)]) -> Self {
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

    pub fn send(&mut self, msg: Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params }));
    }

    /// Send a request and return its id without waiting.
    pub fn start(&mut self, method: &str, params: Value) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        id
    }

    /// Wait for the response to `id`, answering permission prompts with `allow`.
    pub fn wait(&mut self, id: u64, allow: Option<&str>) -> Value {
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

    /// Keep reading until a message matches `pred` (e.g. a notification that
    /// follows a response).
    pub fn wait_for(&mut self, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
        if let Some(m) = self.seen.iter().find(|m| pred(m)) {
            return m.clone();
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = self.rx.recv_timeout(left)
                .unwrap_or_else(|_| panic!("never saw {what}; seen: {:#?}", self.seen));
            self.seen.push(msg.clone());
            if pred(&msg) { return msg; }
        }
    }

    /// Run a slash command (or any prompt) and return the streamed reply text.
    pub fn say(&mut self, sid: &str, text: &str) -> String {
        let before = self.seen.len();
        let r = self.request("session/prompt", Self::prompt_params(sid, text));
        assert_eq!(r["result"]["stopReason"], "end_turn", "{text}: {r}");
        self.seen[before..].iter()
            .filter(|m| m["method"] == "session/update" && m["params"]["update"]["sessionUpdate"] == "agent_message_chunk")
            .filter_map(|m| m["params"]["update"]["content"]["text"].as_str())
            .collect()
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.start(method, params);
        self.wait(id, None)
    }

    pub fn new_session(&mut self) -> String {
        let cwd = self.project.path().to_string_lossy().to_string();
        let r = self.request("session/new", json!({ "cwd": cwd, "mcpServers": [] }));
        r["result"]["sessionId"].as_str().unwrap_or_else(|| panic!("session/new failed: {r}")).to_string()
    }

    pub fn prompt_params(sid: &str, text: &str) -> Value {
        json!({ "sessionId": sid, "prompt": [{ "type": "text", "text": text }] })
    }

    pub fn updates(&self, kind: &str) -> Vec<&Value> {
        self.seen.iter()
            .filter(|m| m["method"] == "session/update" && m["params"]["update"]["sessionUpdate"] == kind)
            .map(|m| &m["params"]["update"])
            .collect()
    }

    pub fn message_text(&self) -> String {
        self.updates("agent_message_chunk").iter()
            .filter_map(|u| u["content"]["text"].as_str())
            .collect()
    }

    /// Stop the agent and hand back its HOME and project dirs for a restart.
    pub fn finish(self) -> (tempfile::TempDir, tempfile::TempDir) {
        drop(self._child);
        (self._home, self.project)
    }
}

/// Kills the agent when a test ends — including when it panics.
pub struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Predicate for `wait_for`: a `session/update` of the given kind.
pub fn is_update(kind: &'static str) -> impl Fn(&Value) -> bool {
    move |m| m["method"] == "session/update" && m["params"]["update"]["sessionUpdate"] == kind
}
