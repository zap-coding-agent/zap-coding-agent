//! `zap acp` — run zap as an Agent Client Protocol (ACP) agent over stdio, so
//! ACP clients (Zed, JetBrains, VS Code extensions, Neovim, Emacs) can drive it.
//!
//! Speaks ACP v1 only; v2 is still a draft. See `docs/roadmap/acp.md`.

pub mod stdio;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, Implementation, InitializeRequest, InitializeResponse,
};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::{Agent, ByteStreams};
use anyhow::Result;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Highest protocol version zap speaks. Per the spec, an agent answers with the
/// client's version when it supports it, otherwise with its own latest.
fn negotiate(requested: ProtocolVersion) -> ProtocolVersion {
    requested.min(ProtocolVersion::V1)
}

fn initialize_response(req: &InitializeRequest) -> InitializeResponse {
    InitializeResponse::new(negotiate(req.protocol_version))
        .agent_capabilities(AgentCapabilities::new())
        .agent_info(Implementation::new("zap", VERSION).title("Zap"))
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

    Agent
        .builder()
        .name("zap")
        .on_receive_request(
            async move |req: InitializeRequest, responder, _connection| {
                responder.respond(initialize_response(&req))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(transport)
        .await
        .map_err(|e| anyhow::anyhow!("ACP connection error: {e}"))
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
        let json = serde_json::to_value(initialize_response(&req)).unwrap();
        assert_eq!(json["protocolVersion"], 1);
        assert_eq!(json["agentInfo"]["name"], "zap");
        assert_eq!(json["agentInfo"]["version"], VERSION);
    }
}
