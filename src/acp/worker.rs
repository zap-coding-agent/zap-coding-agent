//! The zap side of an ACP connection: a dedicated thread that owns every
//! `Session` and runs prompt turns.
//!
//! zap reports progress through the process-global `tui::channel` (events and
//! permission requests), so this worker installs itself as that channel's
//! consumer — exactly what the TUI does — and forwards to the client through a
//! [`Peer`]. Because the channel is global, prompts are serialized: one turn
//! runs at a time per process, across all sessions.

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
            Command::Prompt { session_id, prompt, reply } => {
                let r = match sessions.get_mut(&session_id) {
                    None => Err(anyhow!("unknown session {session_id}")),
                    Some(entry) => {
                        run_prompt(&peer, &session_id, entry, &prompt, &mut ev_rx, &mut cancel_rx).await
                    }
                };
                // A cancel that raced the end of the turn must not kill the next one.
                while cancel_rx.try_recv().is_ok() {}
                let _ = reply.send(r);
            }
        }
    }
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
    Ok(entry)
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

    let mut tr = Translator::new(entry.cwd.clone());
    let mut perm_poll = tokio::time::interval(Duration::from_millis(25));

    let outcome = {
        let turn = entry.session.handle_user_turn(&text);
        tokio::pin!(turn);
        loop {
            tokio::select! {
                biased;
                Some(id) = cancel_rx.recv() => {
                    if id == session_id { break None; } // dropping `turn` cancels it
                }
                Some(ev) = ev_rx.recv() => {
                    for u in tr.event(&ev) { peer.update(session_id, u); }
                }
                r = &mut turn => break Some(r),
                _ = perm_poll.tick() => {
                    if let Some(req) = channel::take_perm_request() {
                        ask_permission(peer, session_id, &mut tr, req);
                    }
                }
            }
        }
    };

    // Events emitted just before the turn finished.
    while let Ok(ev) = ev_rx.try_recv() {
        for u in tr.event(&ev) { peer.update(session_id, u); }
    }
    let _ = channel::take_perm_request();

    match outcome {
        None => Ok(StopReason::Cancelled),
        Some(Ok(())) => Ok(StopReason::EndTurn),
        Some(Err(e)) => Err(e),
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
