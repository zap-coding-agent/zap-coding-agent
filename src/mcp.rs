/// MCP (Model Context Protocol) client — stdio transport, JSON-RPC 2.0.
///
/// Discovers servers from two locations (merged, project wins on conflict):
///   ~/.zap/mcp.json  — global user-level servers (Jira, GitHub, etc.)
///   .mcp.json        — project-level servers in cwd
///
/// Format (Claude Code–compatible):
/// ```json
/// {
///   "mcpServers": {
///     "my-server": {
///       "command": "node",
///       "args": ["mcp-server.js"],
///       "env": {}
///     }
///   }
/// }
/// ```
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::sync::Mutex;

// ── Config file schema ────────────────────────────────────────────────────────

/// Raw entry as it appears in the JSON — both stdio and SSE/HTTP entries.
/// We use a lenient schema so that files shared with Claude Code / Roo Code
/// (which support SSE/HTTP servers with a `url` instead of `command`) don't
/// fail the whole parse when one entry uses a transport we don't support yet.
#[derive(Debug, Deserialize)]
struct RawServerEntry {
    /// stdio transport — the executable to spawn.
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    pub description: Option<String>,
    /// SSE/HTTP transport — url instead of command (not yet supported by zap).
    pub url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawMcpFile {
    #[serde(rename = "mcpServers", default)]
    servers: std::collections::HashMap<String, RawServerEntry>,
}

#[derive(Debug, Default)]
pub struct McpConfig {
    pub servers: std::collections::HashMap<String, McpServerConfig>,
}

#[derive(Debug)]
pub struct McpServerConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: std::collections::HashMap<String, String>,
    pub description: Option<String>,
}

/// Validate an MCP server config before spawning its process.
/// Accepts known interpreter names and absolute paths; rejects shell metacharacters
/// and path traversal in the command field.
fn validate_mcp_command(cfg: &McpServerConfig) -> Result<()> {
    let cmd = &cfg.command;

    const ALLOWED: &[&str] = &[
        "node", "python", "python3", "npx", "uvx", "deno",
        "ruby", "java", "dotnet", "bun",
    ];

    let is_absolute = std::path::Path::new(cmd).is_absolute();
    let is_known    = ALLOWED.contains(&cmd.as_str());

    if !is_absolute && !is_known {
        anyhow::bail!(
            "mcp: command '{}' is neither an absolute path nor a known interpreter {:?}. \
             Use an absolute path or one of the allowed interpreter names.",
            cmd, ALLOWED
        );
    }

    // Reject shell metacharacters that could enable injection.
    for ch in ['|', '&', ';', '$', '`', '(', ')', '<', '>'] {
        if cmd.contains(ch) {
            anyhow::bail!(
                "mcp: command '{}' contains shell metacharacter '{}' — not allowed.",
                cmd, ch
            );
        }
    }

    if cmd.contains("..") {
        anyhow::bail!("mcp: command '{}' contains path traversal '..'", cmd);
    }

    Ok(())
}

/// Load a single `.mcp.json` file, returning an empty config on any error.
/// Entries without `command` (e.g. SSE/HTTP servers used by other agents)
/// are silently skipped — they can't be spawned via stdio.
pub(crate) fn load_file(path: &std::path::Path) -> McpConfig {
    let s = match std::fs::read_to_string(path) {
        Ok(s)  => s,
        Err(e) => { crate::zap_warn!("could not read {}: {}", path.display(), e); return McpConfig::default(); }
    };
    let raw: RawMcpFile = match serde_json::from_str(&s) {
        Ok(r)  => r,
        Err(e) => { crate::zap_warn!("could not parse {}: {}", path.display(), e); return McpConfig::default(); }
    };
    let mut cfg = McpConfig::default();
    for (name, entry) in raw.servers {
        match entry.command {
            Some(cmd) => {
                cfg.servers.insert(name, McpServerConfig {
                    command: cmd,
                    args: entry.args,
                    env: entry.env,
                    description: entry.description,
                });
            }
            None => {
                // SSE/HTTP server — not yet supported; skip without failing.
                let transport = entry.url
                    .as_deref()
                    .map(|u| format!("url={}", u))
                    .unwrap_or_else(|| "unknown transport".to_string());
                crate::zap_warn!(
                    "mcp: skipping '{}' from {} — no `command` field ({}). \
                     Only stdio servers are supported.",
                    name, path.display(), transport
                );
            }
        }
    }
    cfg
}

/// Load MCP config by merging two locations (project entries override global):
///   1. `~/.zap/mcp.json`  — global user-level servers (Jira, GitHub, etc.)
///   2. `.mcp.json`        — project-level servers in cwd
pub fn load_config() -> McpConfig {
    let global_path = dirs::home_dir()
        .map(|h| h.join(".zap").join("mcp.json"));

    let mut merged = McpConfig::default();

    // Layer 1: global
    if let Some(ref p) = global_path {
        if p.exists() {
            let cfg = load_file(p);
            merged.servers.extend(cfg.servers);
        }
    }

    // Layer 2: project (overrides global if same name)
    let project_path = std::path::Path::new(".mcp.json");
    if project_path.exists() {
        let cfg = load_file(project_path);
        merged.servers.extend(cfg.servers);
    }

    merged
}

// ── Tool definition returned by tools/list ────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

// ── JSON-RPC types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct RpcRequest<'a> {
    jsonrpc: &'a str,
    id: u64,
    method: &'a str,
    params: &'a serde_json::Value,
}

#[derive(Serialize)]
struct RpcNotification<'a> {
    jsonrpc: &'a str,
    method: &'a str,
    params: &'a serde_json::Value,
}

// ── McpServer ─────────────────────────────────────────────────────────────────

pub struct McpServer {
    stdin: tokio::process::ChildStdin,
    lines: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    id: u64,
    _child: Child,  // kept alive to prevent process termination
}

impl McpServer {
    /// Spawn a server subprocess, run the MCP initialisation handshake,
    /// and return the server handle together with its tool list.
    pub async fn connect(cfg: &McpServerConfig) -> Result<(Self, Vec<McpToolDef>)> {
        validate_mcp_command(cfg)?;
        let mut child = tokio::process::Command::new(&cfg.command)
            .args(&cfg.args)
            .envs(&cfg.env)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("mcp: failed to spawn '{}'", cfg.command))?;

        let stdin = child.stdin.take()
            .context("mcp: could not open server stdin")?;
        let stdout = child.stdout.take()
            .context("mcp: could not open server stdout")?;
        let lines = BufReader::new(stdout).lines();

        let mut server = Self { stdin, lines, id: 0, _child: child };

        // Initialise.
        server.call("initialize", &serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "zap", "version": env!("CARGO_PKG_VERSION") }
        })).await.context("mcp: initialize failed")?;

        // Confirm initialisation.
        server.notify("notifications/initialized", &serde_json::json!({})).await?;

        // Discover tools.
        let tools_result = server.call("tools/list", &serde_json::json!({}))
            .await.context("mcp: tools/list failed")?;

        let tools: Vec<McpToolDef> = tools_result["tools"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| serde_json::from_value(v.clone()).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok((server, tools))
    }

    async fn call(&mut self, method: &str, params: &serde_json::Value) -> Result<serde_json::Value> {
        self.id += 1;
        let id = self.id;
        let req = RpcRequest { jsonrpc: "2.0", id, method, params };
        let mut line = serde_json::to_string(&req)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await
            .with_context(|| format!("mcp: write to server failed (method={})", method))?;

        // Read until we see the matching response id.
        loop {
            match self.lines.next_line().await? {
                None => anyhow::bail!("mcp: server disconnected (expected id={})", id),
                Some(resp_line) => {
                    if resp_line.trim().is_empty() { continue; }
                    let resp: serde_json::Value = serde_json::from_str(&resp_line)
                        .with_context(|| format!("mcp: bad JSON from server: {}", &resp_line[..resp_line.len().min(200)]))?;
                    if resp["id"].as_u64() != Some(id) { continue; }  // skip notifications
                    if let Some(err) = resp["error"].as_object() {
                        anyhow::bail!("mcp: server error: {}",
                            err.get("message").and_then(|v| v.as_str()).unwrap_or("?"));
                    }
                    return Ok(resp["result"].clone());
                }
            }
        }
    }

    async fn notify(&mut self, method: &str, params: &serde_json::Value) -> Result<()> {
        let notif = RpcNotification { jsonrpc: "2.0", method, params };
        let mut line = serde_json::to_string(&notif)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        Ok(())
    }

    /// Call a tool by name and return its text output.
    pub async fn call_tool(&mut self, name: &str, arguments: serde_json::Value) -> Result<String> {
        let result = self.call("tools/call", &serde_json::json!({
            "name": name,
            "arguments": arguments
        })).await?;

        let text: Vec<&str> = result["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                if c["type"].as_str() == Some("text") { c["text"].as_str() } else { None }
            })
            .collect();

        if text.is_empty() {
            Ok(result.to_string())
        } else {
            Ok(text.join("\n"))
        }
    }
}

// ── McpTool: wraps one tool from an MCP server ────────────────────────────────

pub struct McpTool {
    pub name: String,
    pub description: String,
    pub schema: serde_json::Value,
    pub server: Arc<Mutex<McpServer>>,
}

#[async_trait::async_trait]
impl crate::tools::Tool for McpTool {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.description }
    fn input_schema(&self) -> serde_json::Value { self.schema.clone() }
    fn permission_context(&self, input: &serde_json::Value) -> String {
        format!("MCP {} · {}", self.name, serde_json::to_string(input).unwrap_or_default())
    }
    async fn execute(&self, input: serde_json::Value) -> anyhow::Result<String> {
        let mut srv = self.server.lock().await;
        srv.call_tool(&self.name, input).await
    }
}
