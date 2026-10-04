//! The zap side of an ACP connection: a dedicated thread that owns every
//! `Session` and runs prompt turns.
//!
//! zap reports progress through the process-global `tui::channel` (events and
//! permission requests), so this worker installs itself as that channel's
//! consumer — exactly what the TUI does — and forwards to the client through a
//! [`Peer`]. Because the channel is global, prompts are serialized: one turn
//! runs at a time per process, across all sessions.

use super::commands::{self, Next};
use super::stdio::Capture;
use super::translate::{self, Translator};
use crate::config::{Config, PermissionMode};
use crate::session::Session;
use crate::tui::channel::{self, PermissionDecision, PermissionPromptRequest};
use anyhow::{anyhow, Result};
use futures::future::BoxFuture;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// Where the worker sends things back to the client.
pub trait Peer: Send + Sync + 'static {
    /// Send one `session/update` payload (the `update` object).
    fn update(&self, session_id: &str, update: Value);
    /// Ask the client to approve a tool call (the `toolCall` object).
    fn request_permission(&self, session_id: &str, tool_call: Value) -> BoxFuture<'static, PermissionDecision>;
}

/// Error marker the protocol layer maps to ACP's `auth_required`.
#[derive(Debug)]
pub struct AuthRequired;
impl std::fmt::Display for AuthRequired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no LLM provider configured — run `zap` in a terminal and pick one with /provider")
    }
}
impl std::error::Error for AuthRequired {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    Cancelled,
}

/// ACP session modes, mapped onto zap's permission modes.
pub const MODES: [(&str, &str, &str); 3] = [
    ("ask", "Ask", "Ask before edits and shell commands"),
    ("auto", "Auto", "Run every tool without asking"),
    ("read-only", "Read-only", "Refuse all edits and shell commands"),
];

pub fn mode_id(mode: &PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Ask => "ask",
        PermissionMode::Auto => "auto",
        PermissionMode::Deny => "read-only",
    }
}

fn parse_mode(id: &str) -> Option<PermissionMode> {
    match id {
        "ask" => Some(PermissionMode::Ask),
        "auto" => Some(PermissionMode::Auto),
        "read-only" => Some(PermissionMode::Deny),
        _ => None,
    }
}

/// ACP `modes` object (`SessionModeState`) for a session in `current` mode.
pub fn modes_json(current: &str) -> Value {
    json!({
        "currentModeId": current,
        "availableModes": MODES.iter()
            .map(|(id, name, desc)| json!({ "id": id, "name": name, "description": desc }))
            .collect::<Vec<_>>(),
    })
}

pub struct Opened {
    pub session_id: String,
    pub mode: &'static str,
}

type Reply<T> = oneshot::Sender<Result<T>>;

pub enum Command {
    New { cwd: PathBuf, mcp_servers: Vec<Value>, reply: Reply<Opened> },
    Load { session_id: String, cwd: PathBuf, mcp_servers: Vec<Value>, reply: Reply<Opened> },
    Prompt { session_id: String, prompt: Vec<Value>, reply: Reply<StopReason> },
    SetMode { session_id: String, mode: String, reply: Reply<()> },
    /// The `available_commands_update` payload for a session.
    Commands { session_id: String, reply: Reply<Value> },
    /// Saved sessions for a project directory, as ACP `SessionInfo` objects.
    List { cwd: Option<PathBuf>, reply: Reply<Vec<Value>> },
    /// The client disconnected: run session-end hooks before the process exits.
    Shutdown { reply: Reply<()> },
}

#[derive(Clone)]
pub struct Handle {
    cmd: mpsc::UnboundedSender<Command>,
    cancel: mpsc::UnboundedSender<String>,
}

impl Handle {
    pub fn spawn(peer: Arc<dyn Peer>) -> Self {
        let (cmd, cmd_rx) = mpsc::unbounded_channel();
        let (cancel, cancel_rx) = mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("zap-acp-worker".into())
            .spawn(move || {
                // Multi-threaded: zap's background indexer uses `block_in_place`,
                // which panics on a current-thread runtime.
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("ACP worker runtime");
                rt.block_on(run(peer, cmd_rx, cancel_rx));
            })
            .expect("spawn ACP worker thread");
        Self { cmd, cancel }
    }

    pub async fn call<T>(&self, make: impl FnOnce(Reply<T>) -> Command) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.cmd.send(make(tx)).map_err(|_| anyhow!("zap worker stopped"))?;
        rx.await.map_err(|_| anyhow!("zap worker dropped the request"))?
    }

    /// Tell the worker the client is gone; waits briefly for session-end hooks.
    pub async fn shutdown(&self) {
        let call = self.call(|reply| Command::Shutdown { reply });
        let _ = tokio::time::timeout(Duration::from_secs(3), call).await;
    }

    /// Cancel the running turn of `session_id`, if any. Bypasses the command
    /// queue so it reaches a turn that is already running.
    pub fn cancel(&self, session_id: &str) {
        let _ = self.cancel.send(session_id.to_string());
    }
}

struct Entry {
    session: Session,
    cwd: PathBuf,
}

async fn run(
    peer: Arc<dyn Peer>,
    mut cmd_rx: mpsc::UnboundedReceiver<Command>,
    mut cancel_rx: mpsc::UnboundedReceiver<String>,
) {
    let (ev_tx, mut ev_rx) = mpsc::unbounded_channel();
    channel::set_tui_sender(ev_tx);
    channel::init_perm_channel();
    channel::init_btw_queue();

    let mut sessions: HashMap<String, Entry> = HashMap::new();

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            Command::New { cwd, mcp_servers, reply } => {
                let r = open(&cwd, &mcp_servers).await.map(|entry| {
                    let opened = Opened {
                        session_id: entry.session.session_id.to_string(),
                        mode: mode_id(&entry.session.permissions.mode),
                    };
                    sessions.insert(opened.session_id.clone(), entry);
                    opened
                });
                let _ = reply.send(r);
            }
            Command::Load { session_id, cwd, mcp_servers, reply } => {
                let r = load(&*peer, &session_id, &cwd, &mcp_servers).await.map(|entry| {
                    let opened = Opened { session_id: session_id.clone(), mode: mode_id(&entry.session.permissions.mode) };
                    sessions.insert(session_id, entry);
                    opened
                });
                let _ = reply.send(r);
            }
            Command::SetMode { session_id, mode, reply } => {
                let r = match (sessions.get_mut(&session_id), parse_mode(&mode)) {
                    (None, _) => Err(anyhow!("unknown session {session_id}")),
                    (_, None) => Err(anyhow!("unknown mode {mode}")),
                    (Some(e), Some(m)) => {
                        e.session.permissions.mode = m;
                        Ok(())
                    }
                };
                let _ = reply.send(r);
            }
            Command::Commands { session_id, reply } => {
                let r = sessions.get(&session_id)
                    .map(|e| commands::available(&e.session))
                    .ok_or_else(|| anyhow!("unknown session {session_id}"));
                let _ = reply.send(r);
            }
            Command::List { cwd, reply } => {
                let _ = reply.send(list_sessions(cwd));
            }
            Command::Shutdown { reply } => {
                for entry in sessions.values() {
                    entry.session.save_context();
                    entry.session.hooks.fire_session_end();
                }
                let _ = reply.send(Ok(()));
                return;
            }
            Command::Prompt { session_id, prompt, reply } => {
                let r = match sessions.get_mut(&session_id) {
                    None => Err(anyhow!("unknown session {session_id}")),
                    Some(entry) => {
                        // A panic in a turn or command must not take the worker
                        // (and with it every other thread in the editor) down.
                        use futures::FutureExt as _;
                        let work = run_prompt(&peer, &session_id, entry, &prompt, &mut ev_rx, &mut cancel_rx);
                        match std::panic::AssertUnwindSafe(work).catch_unwind().await {
                            Ok(r) => r,
                            Err(panic) => {
                                let msg = panic.downcast_ref::<&str>().map(|s| s.to_string())
                                    .or_else(|| panic.downcast_ref::<String>().cloned())
                                    .unwrap_or_else(|| "unknown panic".to_string());
                                Err(anyhow!("zap hit an internal error: {msg}"))
                            }
                        }
                    }
                };
                // A cancel that raced the end of the turn must not kill the next one.
                while cancel_rx.try_recv().is_ok() {}
                let _ = reply.send(r);
            }
        }
    }
}

fn list_sessions(cwd: Option<PathBuf>) -> Result<Vec<Value>> {
    let cwd = match cwd {
        Some(c) => c,
        None => std::env::current_dir()?,
    };
    // Sessions are keyed by the resolved cwd (what `current_dir()` reported
    // when they were created); a client may send a symlinked spelling.
    let cwd = std::fs::canonicalize(&cwd).unwrap_or(cwd).display().to_string();
    let store = crate::persistence::init()?;
    Ok(store.recent_sessions_for_cwd(&cwd, 50)?
        .into_iter()
        .map(|(id, goal, _model, created_at)| json!({
            "sessionId": id.to_string(),
            "cwd": cwd,
            "title": goal,
            "updatedAt": created_at,
        }))
        .collect())
}

fn load_config() -> Result<Config> {
    let mut config = Config::load()?;
    if config.no_provider_configured() {
        return Err(AuthRequired.into());
    }
    // Suppress startup banners; there is no terminal to show them in.
    config.tui_mode = true;
    Ok(config)
}

async fn open(cwd: &PathBuf, mcp_servers: &[Value]) -> Result<Entry> {
    // Session::new reads the project (index, skills, ZAP.md) from the cwd.
    std::env::set_current_dir(cwd).map_err(|e| anyhow!("cannot use cwd {}: {e}", cwd.display()))?;
    let config = load_config()?;
    let mut session = Session::new(&config).await?;
    // The TUI auto-resumes the previous conversation into a new session (and
    // offers /new to drop it). An editor thread is a fresh conversation the
    // user can see in full, so hidden history from another thread must not
    // ride along; the "last session" handoff in the system prompt stays.
    session.messages.clear();

    let servers = translate::mcp_servers(mcp_servers);
    if !servers.is_empty() {
        session.tools.load_mcp_lazy(crate::mcp::McpConfig {
            servers: servers.into_iter().collect(),
            had_config: true,
        });
        session.tool_defs = session.tools.tool_definitions_filtered(&session.config.disabled_tools);
    }
    Ok(Entry { session, cwd: cwd.clone() })
}

async fn load(peer: &dyn Peer, session_id: &str, cwd: &PathBuf, mcp_servers: &[Value]) -> Result<Entry> {
    let id: i64 = session_id.parse().map_err(|_| anyhow!("unknown session {session_id}"))?;
    let mut entry = open(cwd, mcp_servers).await?;
    if entry.session.store.get_session_goal(id).is_none() {
        return Err(anyhow!("unknown session {session_id}"));
    }
    // A session that was created but never prompted has no saved messages yet.
    let messages: Vec<crate::llm_client::Message> = match entry.session.store.load_messages(id)? {
        Some(json) => serde_json::from_str(&json)?,
        None => Vec::new(),
    };

    // The spec requires replaying the whole conversation before responding.
    for update in translate::history(&messages) {
        peer.update(session_id, update);
    }
    entry.session.turn_count = messages.iter().filter(|m| m.role == "user").count();
    entry.session.messages = messages;
    entry.session.session_id = id;
    // Unlike session/new, the client already knows this session id — and
    // everything belonging to a load must arrive before its response.
    peer.update(session_id, commands::available(&entry.session));
    Ok(entry)
}

/// Per-prompt plumbing: where output goes and what can interrupt the work.
struct Io<'a> {
    peer: &'a Arc<dyn Peer>,
    session_id: &'a str,
    tr: Translator,
    ev_rx: &'a mut mpsc::UnboundedReceiver<channel::TuiEvent>,
    cancel_rx: &'a mut mpsc::UnboundedReceiver<String>,
    /// A ```text fence is open for captured terminal output.
    fence_open: bool,
    emitted: bool,
}

impl Io<'_> {
    fn chunk(&mut self, s: &str) {
        self.emitted = true;
        self.peer.update(self.session_id, json!({
            "sessionUpdate": "agent_message_chunk",
            "content": { "type": "text", "text": s },
        }));
    }

    fn close_fence(&mut self) {
        if self.fence_open {
            self.fence_open = false;
            self.chunk("```\n");
        }
    }

    /// Markdown text from zap itself (command results, status lines).
    fn text(&mut self, s: &str) {
        self.close_fence();
        self.chunk(s);
    }

    /// Captured terminal output — kept in a code block so columns stay aligned.
    fn raw(&mut self, s: &str) {
        let s = translate::strip_ansi(s);
        if s.trim().is_empty() && !self.fence_open { return; }
        if !self.fence_open {
            self.fence_open = true;
            self.chunk("```text\n");
        }
        self.chunk(&s);
    }

    fn event(&mut self, ev: &channel::TuiEvent) {
        for u in self.tr.event(ev) {
            self.close_fence();
            self.emitted = true;
            self.peer.update(self.session_id, u);
        }
    }

    /// Run `work` to completion, forwarding events, permission requests and
    /// (when `capture` is set) terminal output as they happen. `None` means the
    /// client cancelled — `work` is dropped, which is how zap cancels a turn.
    async fn drive<T>(
        &mut self,
        work: impl std::future::Future<Output = T>,
        mut capture: Option<&mut Capture>,
    ) -> Option<T> {
        let mut poll = tokio::time::interval(Duration::from_millis(25));
        tokio::pin!(work);
        let outcome = loop {
            tokio::select! {
                biased;
                Some(id) = self.cancel_rx.recv() => {
                    if id == self.session_id { break None; }
                }
                Some(ev) = self.ev_rx.recv() => self.event(&ev),
                r = &mut work => break Some(r),
                _ = poll.tick() => {
                    if let Some(req) = channel::take_perm_request() {
                        self.close_fence();
                        ask_permission(self.peer, self.session_id, &mut self.tr, req);
                    }
                    if let Some(c) = capture.as_deref_mut() {
                        let out = c.read_new(false);
                        if !out.is_empty() { self.raw(&out); }
                    }
                }
            }
        };
        // Events emitted just before the work finished.
        while let Ok(ev) = self.ev_rx.try_recv() { self.event(&ev); }
        let _ = channel::take_perm_request();
        outcome
    }
}

async fn run_prompt(
    peer: &Arc<dyn Peer>,
    session_id: &str,
    entry: &mut Entry,
    prompt: &[Value],
    ev_rx: &mut mpsc::UnboundedReceiver<channel::TuiEvent>,
    cancel_rx: &mut mpsc::UnboundedReceiver<String>,
) -> Result<StopReason> {
    let (text, images) = translate::prompt_input(prompt);
    if text.is_empty() && images.is_empty() {
        return Ok(StopReason::EndTurn);
    }
    std::env::set_current_dir(&entry.cwd)?;
    entry.session.staged_images.extend(images);

    // Leftovers from a previous turn (e.g. one that was cancelled mid-prompt).
    while ev_rx.try_recv().is_ok() {}
    let _ = channel::take_perm_request();

    let mut io = Io {
        peer,
        session_id,
        tr: Translator::new(entry.cwd.clone()),
        ev_rx,
        cancel_rx,
        fence_open: false,
        emitted: false,
    };

    let mode_before = mode_id(&entry.session.permissions.mode);
    let result = if text.starts_with('/') {
        run_slash(&mut io, entry, &text).await
    } else {
        run_turn(&mut io, entry, &text).await
    };
    io.close_fence();

    // The TUI writes .zap/context.md (goal, files touched) when it exits; an
    // editor just drops the connection, so keep it current after every turn.
    if matches!(result, Ok(StopReason::EndTurn)) {
        entry.session.save_context();
    }

    // A command (/permissions) may have changed the mode behind the client's back.
    let mode_after = mode_id(&entry.session.permissions.mode);
    if mode_after != mode_before {
        peer.update(session_id, json!({ "sessionUpdate": "current_mode_update", "currentModeId": mode_after }));
    }
    result
}

async fn run_turn(io: &mut Io<'_>, entry: &mut Entry, text: &str) -> Result<StopReason> {
    match io.drive(entry.session.handle_user_turn(text), None).await {
        None => Ok(StopReason::Cancelled),
        Some(Ok(())) => Ok(StopReason::EndTurn),
        Some(Err(e)) => Err(e),
    }
}

/// A prompt that starts with `/`: a skill, a zap command, or a goal loop.
async fn run_slash(io: &mut Io<'_>, entry: &mut Entry, text: &str) -> Result<StopReason> {
    use crate::tui::commands::{could_be_skill_command, resolve_skill_command};

    // `/<skill-name> …` pins that skill for one turn, like the TUI.
    if could_be_skill_command(text) {
        let names: Vec<String> = entry.session.skills.iter().map(|s| s.name.clone()).collect();
        if let Some(skill) = resolve_skill_command(text, &names) {
            let pinned_here = entry.session.pinned_skills.insert(skill.clone());
            let r = run_turn(io, entry, &text[1..]).await;
            if pinned_here { entry.session.pinned_skills.remove(&skill); }
            return r;
        }
    }

    let next = {
        let mut out = |s: &str| io.text(s);
        commands::run(&mut entry.session, &mut entry.cwd, text, &mut out).await
    };
    match next {
        Next::Done => Ok(StopReason::EndTurn),
        Next::Turn(prompt) => run_turn(io, entry, &prompt).await,
        Next::Goal { condition, max_turns } => {
            for n in 1..=max_turns {
                io.text(&format!("\n\n**Goal — turn {n}/{max_turns}**\n\n"));
                let prompt = commands::goal_prompt(n, max_turns, &condition);
                if run_turn(io, entry, &prompt).await? == StopReason::Cancelled {
                    return Ok(StopReason::Cancelled);
                }
                if commands::goal_done(&entry.session) {
                    io.text(&format!("\n\n✓ Goal complete in {n} turn{}.", if n == 1 { "" } else { "s" }));
                    return Ok(StopReason::EndTurn);
                }
            }
            io.text(&format!("\n\n⏹ Goal stopped: {max_turns} turn limit reached."));
            Ok(StopReason::EndTurn)
        }
        // CLI-style command: it reports through println!, so capture stdout.
        Next::Fallback => {
            let mut capture = Capture::start().ok();
            let config = entry.session.config.clone();
            let done = io.drive(entry.session.handle_slash(text, &config), capture.as_mut()).await;
            if let Some(c) = capture {
                let rest = c.finish();
                if !rest.is_empty() { io.raw(&rest); }
            }
            io.close_fence();
            if done.is_none() {
                return Ok(StopReason::Cancelled);
            }
            if !io.emitted {
                io.text("Done.");
            }
            Ok(StopReason::EndTurn)
        }
    }
}

/// Show the pending tool calls in the client and ask once for the whole batch
/// (zap approves batches all-or-nothing). Runs detached so events keep flowing
/// and a cancel can still interrupt the turn while the user decides.
fn ask_permission(peer: &Arc<dyn Peer>, session_id: &str, tr: &mut Translator, req: PermissionPromptRequest) {
    for (id, name, ctx) in &req.pending {
        peer.update(session_id, tr.announce(id, name, ctx));
    }
    let Some((id, name, ctx)) = req.pending.first() else {
        let _ = req.response_tx.send(PermissionDecision::Deny);
        return;
    };
    let title = if req.pending.len() == 1 {
        format!("{name} {ctx}")
    } else {
        let names: Vec<&str> = req.pending.iter().map(|(_, n, _)| n.as_str()).collect();
        format!("{} operations: {}", req.pending.len(), names.join(", "))
    };
    let tool_call = json!({
        "toolCallId": id,
        "title": title,
        "kind": translate::tool_kind(name),
        "status": "pending",
    });
    let decision = peer.request_permission(session_id, tool_call);
    tokio::spawn(async move {
        let _ = req.response_tx.send(decision.await);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_round_trip() {
        for (id, _, _) in MODES {
            assert_eq!(mode_id(&parse_mode(id).unwrap()), id);
        }
        assert!(parse_mode("yolo").is_none());
        let m = modes_json("ask");
        assert_eq!(m["currentModeId"], "ask");
        assert_eq!(m["availableModes"].as_array().unwrap().len(), 3);
    }
}
