//! `zap acp` — run zap as an Agent Client Protocol (ACP) agent over stdio, so
//! ACP clients (Zed, JetBrains, VS Code extensions, Neovim, Emacs) can drive it.
//!
//! Speaks ACP v1 only; v2 is still a draft. See `docs/roadmap/acp.md`.
//!
//! Layout: `stdio` isolates the protocol from stray output, this module maps
//! JSON-RPC methods onto the `worker` thread that owns zap's sessions, and
//! `translate` holds the pure zap ⇄ ACP conversions.

pub mod stdio;
pub mod translate;
pub mod worker;

use agent_client_protocol::schema::v1::{
    AuthenticateRequest, AuthenticateResponse, CancelNotification, InitializeRequest,
    InitializeResponse, LoadSessionRequest, LoadSessionResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, RequestPermissionRequest,
    SessionNotification, SetSessionModeRequest, SetSessionModeResponse,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo, Error, Responder};
use anyhow::Result;
use futures::future::BoxFuture;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

use crate::tui::channel::PermissionDecision;
use worker::{Command, Handle, Peer, StopReason};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Highest protocol version zap speaks. Per the spec, an agent answers with the
/// client's version when it supports it, otherwise with its own latest.
fn negotiate(requested: ProtocolVersion) -> ProtocolVersion {
    requested.min(ProtocolVersion::V1)
}

/// Build a typed protocol struct from its spec-shaped JSON.
fn typed<T: DeserializeOwned>(v: Value) -> Result<T, Error> {
    serde_json::from_value(v).map_err(|e| Error::internal_error().data(e.to_string()))
}

fn to_json<T: serde::Serialize>(v: &T) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Map a worker error onto an ACP error, keeping the message visible.
fn acp_error(e: anyhow::Error) -> Error {
    if e.downcast_ref::<worker::AuthRequired>().is_some() {
        Error::auth_required().data(e.to_string())
    } else {
        Error::new(-32603, e.to_string())
    }
}

fn initialize_response(req: &InitializeRequest) -> Result<InitializeResponse, Error> {
    // The ACP registry lists only agents with an auth method. zap's "login" is
    // picking a provider, which its terminal UI already does — but the spec
    // forbids offering a terminal method to clients that can't run one.
    let auth_methods = if to_json(req)["clientCapabilities"]["auth"]["terminal"] == true {
        json!([{
            "type": "terminal",
            "id": "zap-setup",
            "name": "Set up zap",
            "description": "Pick an LLM provider and model in zap's terminal UI",
            "args": [],
        }])
    } else {
        json!([])
    };
    typed(json!({
        "protocolVersion": negotiate(req.protocol_version),
        "agentCapabilities": {
            "loadSession": true,
            "promptCapabilities": { "image": true, "embeddedContext": true },
            "mcpCapabilities": { "http": false, "sse": false },
        },
        "authMethods": auth_methods,
        "agentInfo": { "name": "zap", "title": "Zap", "version": VERSION },
    }))
}

/// [`Peer`] backed by the live ACP connection.
struct AcpPeer {
    cx: ConnectionTo<Client>,
}

impl Peer for AcpPeer {
    fn update(&self, session_id: &str, update: Value) {
        match typed::<SessionNotification>(json!({ "sessionId": session_id, "update": update })) {
            Ok(n) => { let _ = self.cx.send_notification(n); }
            Err(e) => tracing::warn!("acp: dropped malformed session/update: {e:?}"),
        }
    }

    fn request_permission(&self, session_id: &str, tool_call: Value) -> BoxFuture<'static, PermissionDecision> {
        let req = typed::<RequestPermissionRequest>(json!({
            "sessionId": session_id,
            "toolCall": tool_call,
            "options": [
                { "optionId": "allow_once", "name": "Allow", "kind": "allow_once" },
                { "optionId": "allow_always", "name": "Always allow", "kind": "allow_always" },
                { "optionId": "reject_once", "name": "Reject", "kind": "reject_once" },
            ],
        }));
        let cx = self.cx.clone();
        Box::pin(async move {
            let Ok(req) = req else { return PermissionDecision::Deny };
            match cx.send_request(req).block_task().await {
                Ok(resp) => decision(&to_json(&resp)),
                Err(_) => PermissionDecision::Deny,
            }
        })
    }
}

/// `RequestPermissionResponse` JSON → zap decision. Anything but an explicit
/// allow (including `cancelled`) denies.
fn decision(resp: &Value) -> PermissionDecision {
    match (resp["outcome"]["outcome"].as_str(), resp["outcome"]["optionId"].as_str()) {
        (Some("selected"), Some("allow_once")) => PermissionDecision::Allow,
        (Some("selected"), Some("allow_always")) => PermissionDecision::Always,
        _ => PermissionDecision::Deny,
    }
}

/// The worker is started on the first session request, when the connection
/// handle is in hand. One per process.
#[derive(Clone, Default)]
struct State {
    worker: Arc<OnceLock<Handle>>,
}

impl State {
    fn worker(&self, cx: &ConnectionTo<Client>) -> Handle {
        self.worker
            .get_or_init(|| Handle::spawn(Arc::new(AcpPeer { cx: cx.clone() })))
            .clone()
    }
}

/// Run `work` outside the dispatch loop (so cancels and permission replies
/// keep flowing) and answer `responder` with its result.
fn respond_later<T, F>(cx: &ConnectionTo<Client>, responder: Responder<T>, work: F) -> Result<(), Error>
where
    T: agent_client_protocol::JsonRpcResponse + Send + 'static,
    F: std::future::Future<Output = Result<T, Error>> + Send + 'static,
{
    cx.spawn(async move {
        match work.await {
            Ok(v) => responder.respond(v),
            Err(e) => responder.respond_with_error(e),
        }
    })
}

pub async fn run() -> Result<()> {
    // Must happen before anything can print — see `stdio` module docs.
    let io = stdio::isolate()?;
    std::env::set_var("NO_COLOR", "1");

    // Test hook: prove that stray prints and child processes miss the protocol.
    if std::env::var_os("ZAP_ACP_TEST_STRAY_OUTPUT").is_some() {
        println!("stray println");
        let _ = std::process::Command::new(if cfg!(windows) { "cmd" } else { "sh" })
            .args(if cfg!(windows) { ["/C", "echo stray child"] } else { ["-c", "echo stray child"] })
            .status();
    }

    let transport = ByteStreams::new(
        tokio::fs::File::from_std(io.stdout).compat_write(),
        tokio::fs::File::from_std(io.stdin).compat(),
    );
    let state = State::default();
    let (s1, s2, s3, s4, s5) = (state.clone(), state.clone(), state.clone(), state.clone(), state);

    let result = Agent
        .builder()
        .name("zap")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _cx| {
                match initialize_response(&req) {
                    Ok(r) => responder.respond(r),
                    Err(e) => responder.respond_with_error(e),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            // Terminal auth runs zap interactively; nothing to do in-protocol.
            async move |_req: AuthenticateRequest, responder, _cx| {
                responder.respond(typed::<AuthenticateResponse>(json!({}))?)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: NewSessionRequest, responder, cx| {
                let worker = s1.worker(&cx);
                let req = to_json(&req);
                respond_later(&cx, responder, async move {
                    let opened = worker
                        .call(|reply| Command::New {
                            cwd: req["cwd"].as_str().unwrap_or(".").into(),
                            mcp_servers: req["mcpServers"].as_array().cloned().unwrap_or_default(),
                            reply,
                        })
                        .await
                        .map_err(acp_error)?;
                    typed::<NewSessionResponse>(json!({
                        "sessionId": opened.session_id,
                        "modes": worker::modes_json(opened.mode),
                    }))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: LoadSessionRequest, responder, cx| {
                let worker = s2.worker(&cx);
                let req = to_json(&req);
                respond_later(&cx, responder, async move {
                    let opened = worker
                        .call(|reply| Command::Load {
                            session_id: req["sessionId"].as_str().unwrap_or_default().to_string(),
                            cwd: req["cwd"].as_str().unwrap_or(".").into(),
                            mcp_servers: req["mcpServers"].as_array().cloned().unwrap_or_default(),
                            reply,
                        })
                        .await
                        .map_err(acp_error)?;
                    typed::<LoadSessionResponse>(json!({ "modes": worker::modes_json(opened.mode) }))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, cx| {
                let worker = s3.worker(&cx);
                let req = to_json(&req);
                respond_later(&cx, responder, async move {
                    let stop = worker
                        .call(|reply| Command::Prompt {
                            session_id: req["sessionId"].as_str().unwrap_or_default().to_string(),
                            prompt: req["prompt"].as_array().cloned().unwrap_or_default(),
                            reply,
                        })
                        .await
                        .map_err(acp_error)?;
                    let stop = match stop {
                        StopReason::EndTurn => "end_turn",
                        StopReason::Cancelled => "cancelled",
                    };
                    typed::<PromptResponse>(json!({ "stopReason": stop }))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: SetSessionModeRequest, responder, cx| {
                let worker = s4.worker(&cx);
                let req = to_json(&req);
                respond_later(&cx, responder, async move {
                    worker
                        .call(|reply| Command::SetMode {
                            session_id: req["sessionId"].as_str().unwrap_or_default().to_string(),
                            mode: req["modeId"].as_str().unwrap_or_default().to_string(),
                            reply,
                        })
                        .await
                        .map_err(acp_error)?;
                    typed::<SetSessionModeResponse>(json!({}))
                })
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            async move |n: CancelNotification, cx| {
                s5.worker(&cx).cancel(to_json(&n)["sessionId"].as_str().unwrap_or_default());
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(transport)
        .await;

    // The worker thread and the blocking stdin reader would keep the runtime
    // alive; the client is gone, so leave now.
    if let Err(e) = &result {
        tracing::error!("ACP connection error: {e}");
    }
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiate_echoes_v1_and_caps_newer_versions() {
        assert_eq!(negotiate(ProtocolVersion::V1), ProtocolVersion::V1);
        assert_eq!(negotiate(ProtocolVersion::from(2)), ProtocolVersion::V1);
    }

    #[test]
    fn initialize_response_identifies_zap() {
        let req = InitializeRequest::new(ProtocolVersion::from(2));
        let json = to_json(&initialize_response(&req).unwrap());
        assert_eq!(json["protocolVersion"], 1);
        assert_eq!(json["agentInfo"]["name"], "zap");
        assert_eq!(json["agentInfo"]["version"], VERSION);
        assert_eq!(json["agentCapabilities"]["loadSession"], true);
    }

    #[test]
    fn terminal_setup_offered_only_to_clients_that_support_it() {
        let init = |caps: Value| -> Value {
            let req: InitializeRequest =
                serde_json::from_value(json!({ "protocolVersion": 1, "clientCapabilities": caps })).unwrap();
            to_json(&initialize_response(&req).unwrap())
        };
        let without = init(json!({}));
        assert!(without["authMethods"].as_array().is_none_or(|a| a.is_empty()), "{without}");
        let with = init(json!({ "auth": { "terminal": true } }));
        assert_eq!(with["authMethods"][0]["type"], "terminal");
        assert_eq!(with["authMethods"][0]["id"], "zap-setup");
    }

    #[test]
    fn permission_decision_mapping() {
        let sel = |id: &str| json!({ "outcome": { "outcome": "selected", "optionId": id } });
        assert_eq!(decision(&sel("allow_once")), PermissionDecision::Allow);
        assert_eq!(decision(&sel("allow_always")), PermissionDecision::Always);
        assert_eq!(decision(&sel("reject_once")), PermissionDecision::Deny);
        assert_eq!(decision(&json!({ "outcome": { "outcome": "cancelled" } })), PermissionDecision::Deny);
    }

    #[test]
    fn spec_shaped_updates_deserialize_into_typed_notifications() {
        // Every shape the translator emits must survive the typed conversion.
        let updates = [
            json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": "x" } }),
            json!({ "sessionUpdate": "tool_call", "toolCallId": "t", "title": "edit_file a", "kind": "edit",
                    "status": "in_progress", "rawInput": {}, "locations": [{ "path": "/a", "line": 2 }],
                    "content": [{ "type": "diff", "path": "/a", "oldText": "x", "newText": "y" }] }),
            json!({ "sessionUpdate": "tool_call_update", "toolCallId": "t", "status": "completed",
                    "content": [{ "type": "content", "content": { "type": "text", "text": "ok" } }] }),
            json!({ "sessionUpdate": "plan", "entries": [{ "content": "a", "priority": "high", "status": "completed" }] }),
        ];
        for u in updates {
            typed::<SessionNotification>(json!({ "sessionId": "1", "update": u.clone() }))
                .unwrap_or_else(|e| panic!("{u} failed: {e:?}"));
        }
        typed::<NewSessionResponse>(json!({ "sessionId": "1", "modes": worker::modes_json("ask") })).unwrap();
        typed::<PromptResponse>(json!({ "stopReason": "cancelled" })).unwrap();
    }
}
