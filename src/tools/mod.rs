/// Tool trait, registry, and MCP integration. Tool implementations live in submodules.
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

pub mod agent;
pub mod file;
pub mod search;
pub mod shell;
pub mod web;

pub use agent::SpawnAgentTool;

// ── Tool trait ────────────────────────────────────────────────────────────────

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    fn permission_context(&self, input: &serde_json::Value) -> String;
    async fn execute(&self, input: serde_json::Value) -> Result<String>;

    /// Returns the filesystem path this tool writes to, if any.
    /// The session uses this to trigger incremental code re-indexing after mutations.
    fn affected_path<'a>(&self, _input: &'a serde_json::Value) -> Option<&'a str> {
        None
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// MCP servers that are configured but not yet spawned.
    pending_mcp: HashMap<String, crate::mcp::McpServerConfig>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        use file::{BatchEditTool, EditFileTool, GlobReadTool, ReadFileTool, UndoEditTool, WriteFileTool};
        use search::{CodeMapTool, FindDefinitionTool, FindReferencesTool, SearchCodeTool};
        use shell::{GitStatusTool, ListDirectoryTool, ShellTool};
        use web::{WebFetchTool, WebSearchTool};

        let mut r = Self { tools: HashMap::new(), pending_mcp: HashMap::new() };
        r.register(Arc::new(ReadFileTool));
        r.register(Arc::new(EditFileTool));
        r.register(Arc::new(WriteFileTool));
        r.register(Arc::new(BatchEditTool));
        r.register(Arc::new(UndoEditTool));
        r.register(Arc::new(ShellTool));
        r.register(Arc::new(GitStatusTool));
        r.register(Arc::new(SearchCodeTool));
        r.register(Arc::new(FindDefinitionTool));
        r.register(Arc::new(FindReferencesTool));
        r.register(Arc::new(ListDirectoryTool));
        r.register(Arc::new(GlobReadTool));
        r.register(Arc::new(CodeMapTool));
        r.register(Arc::new(WebFetchTool));
        r.register(Arc::new(WebSearchTool));
        r
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    // ── Lazy MCP loading ──────────────────────────────────────────────────────

    /// Store MCP server configs without spawning any subprocesses.
    /// Call this at startup instead of `register_mcp_servers`.
    pub fn load_mcp_lazy(&mut self, cfg: crate::mcp::McpConfig) {
        for (name, server_cfg) in cfg.servers {
            self.pending_mcp.insert(name, server_cfg);
        }
    }

    /// Names and optional descriptions of servers not yet connected.
    pub fn pending_mcp_servers(&self) -> Vec<(&str, Option<&str>)> {
        self.pending_mcp
            .iter()
            .map(|(n, cfg)| (n.as_str(), cfg.description.as_deref()))
            .collect()
    }

    pub fn has_pending_mcp(&self) -> bool {
        !self.pending_mcp.is_empty()
    }

    /// Spawn a pending MCP server, run the handshake, register its tools, and
    /// return a summary string for the LLM ("Connected. Tools: read, write, …").
    pub async fn connect_mcp(&mut self, server_name: &str) -> Result<String> {
        let cfg = self.pending_mcp.remove(server_name).ok_or_else(|| {
            let available: Vec<&str> = self.pending_mcp.keys().map(|s| s.as_str()).collect();
            if available.is_empty() {
                anyhow::anyhow!("MCP server '{}' not found (all servers already connected)", server_name)
            } else {
                anyhow::anyhow!(
                    "MCP server '{}' not found — available: {}",
                    server_name,
                    available.join(", ")
                )
            }
        })?;

        let (server, tools) = crate::mcp::McpServer::connect(&cfg).await?;
        let srv = Arc::new(tokio::sync::Mutex::new(server));
        let mut names: Vec<String> = Vec::new();
        for tool_def in tools {
            names.push(tool_def.name.clone());
            tracing::info!(server = %server_name, tool = %tool_def.name, "registered MCP tool");
            self.register(Arc::new(crate::mcp::McpTool {
                name: tool_def.name,
                description: tool_def.description.unwrap_or_default(),
                schema: tool_def.input_schema,
                server: srv.clone(),
            }));
        }
        Ok(format!("Connected to '{}'. Tools: {}", server_name, names.join(", ")))
    }

    // ── Tool definitions ──────────────────────────────────────────────────────

    pub fn tool_definitions(&self) -> Vec<serde_json::Value> {
        let mut defs: Vec<serde_json::Value> = self
            .tools
            .values()
            .map(|t| serde_json::json!({
                "name": t.name(),
                "description": t.description(),
                "input_schema": t.input_schema(),
            }))
            .collect();
        defs.sort_by_key(|d| d["name"].as_str().unwrap_or("").to_string());

        // Append a synthetic mcp_connect tool when there are unconnected servers.
        // It disappears from the list once all servers have been connected.
        if !self.pending_mcp.is_empty() {
            let server_list = {
                let mut names: Vec<&str> = self.pending_mcp.keys().map(|s| s.as_str()).collect();
                names.sort_unstable();
                names.join(", ")
            };
            defs.push(serde_json::json!({
                "name": "mcp_connect",
                "description": format!(
                    "Connect to an MCP server and load its tools into the session. \
                     Call this before using any tool from that server. \
                     Available servers: {server_list}."
                ),
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "server": {
                            "type": "string",
                            "description": format!("MCP server name. One of: {server_list}")
                        }
                    },
                    "required": ["server"]
                }
            }));
        }

        defs
    }
}
