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

    /// Whether the tool's raw output should be printed inline to the terminal.
    /// True for shell where seeing the raw result is expected.
    /// File/search/code tools keep silent — the LLM summarises.
    fn shows_inline_output(&self) -> bool {
        false
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// MCP servers that are configured but not yet spawned.
    pending_mcp: HashMap<String, crate::mcp::McpServerConfig>,
    /// Names of tools registered from MCP servers — used for permission gating.
    mcp_tool_names: std::collections::HashSet<String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        use file::{BatchEditTool, EditFileTool, GlobReadTool, ReadFileTool, UndoEditTool, WriteFileTool};
        use search::{CodeMapTool, FindDefinitionTool, FindReferencesTool, SearchCodeTool};
        use shell::{ListDirectoryTool, ShellTool};
        use web::{WebFetchTool, WebSearchTool};

        let mut r = Self { tools: HashMap::new(), pending_mcp: HashMap::new(), mcp_tool_names: std::collections::HashSet::new() };
        r.register(Arc::new(ReadFileTool));
        r.register(Arc::new(EditFileTool));
        r.register(Arc::new(WriteFileTool));
        r.register(Arc::new(BatchEditTool));
        r.register(Arc::new(UndoEditTool));
        r.register(Arc::new(ShellTool));
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

    /// Returns true if `name` was registered from an MCP server.
    pub fn is_mcp_tool(&self, name: &str) -> bool {
        self.mcp_tool_names.contains(name)
    }

    // ── MCP loading ───────────────────────────────────────────────────────────

    /// Store MCP server configs without spawning any subprocesses yet.
    pub fn load_mcp_lazy(&mut self, cfg: crate::mcp::McpConfig) {
        for (name, server_cfg) in cfg.servers {
            self.pending_mcp.insert(name, server_cfg);
        }
    }

    /// Eagerly connect all pending MCP servers at startup.
    /// Returns one entry per server: `(server_name, Ok(tool_names) | Err(reason))`.
    pub async fn connect_all_mcp(&mut self) -> Vec<(String, Result<Vec<String>>)> {
        let names: Vec<String> = self.pending_mcp.keys().cloned().collect();
        let mut results = Vec::new();
        for name in names {
            let outcome = self.connect_mcp_inner(&name).await;
            results.push((name, outcome));
        }
        results
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

    /// Connect one pending MCP server and register its tools. Returns list of tool names.
    async fn connect_mcp_inner(&mut self, server_name: &str) -> Result<Vec<String>> {
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

        let (server, tools) = crate::mcp::McpServer::connect(server_name, &cfg).await?;
        let srv = Arc::new(tokio::sync::Mutex::new(server));
        let mut names: Vec<String> = Vec::new();
        for tool_def in tools {
            names.push(tool_def.name.clone());
            self.mcp_tool_names.insert(tool_def.name.clone());
            tracing::info!(server = %server_name, tool = %tool_def.name, "registered MCP tool");
            self.register(Arc::new(crate::mcp::McpTool {
                name: tool_def.name,
                description: tool_def.description.unwrap_or_default(),
                schema: tool_def.input_schema,
                server: srv.clone(),
            }));
        }
        Ok(names)
    }

    /// Spawn a pending MCP server, run the handshake, register its tools, and
    /// return a summary string for the LLM tool result.
    pub async fn connect_mcp(&mut self, server_name: &str) -> Result<String> {
        let names = self.connect_mcp_inner(server_name).await?;
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
        // Each pending server gets a line with its description + tools_hint so the
        // LLM knows when to connect it without paying for actual tool definitions.
        // Disappears once all servers have been connected.
        if !self.pending_mcp.is_empty() {
            let mut entries: Vec<&str> = self.pending_mcp.keys().map(|s| s.as_str()).collect();
            entries.sort_unstable();

            let server_lines: String = entries.iter().map(|name| {
                let cfg = &self.pending_mcp[*name];
                let mut line = format!("  - {}", name);
                if let Some(desc) = &cfg.description {
                    line.push_str(&format!(": {}", desc));
                }
                if let Some(hint) = &cfg.tools_hint {
                    line.push_str(&format!(" [tools: {}]", hint));
                }
                line
            }).collect::<Vec<_>>().join("\n");

            let server_list = entries.join(", ");

            defs.push(serde_json::json!({
                "name": "mcp_connect",
                "description": format!(
                    "Connect to an MCP server and register its tools into this session. \
                     Call this before using any tool from that server. \
                     Available servers:\n{server_lines}"
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod mcp_lazy_tests {
    use super::*;
    use crate::mcp::{McpConfig, McpServerConfig};
    use std::collections::HashMap;

    fn make_cfg(description: Option<&str>, tools_hint: Option<&str>) -> McpServerConfig {
        McpServerConfig {
            command: "npx".to_string(),
            args: vec![],
            env: HashMap::new(),
            description: description.map(|s| s.to_string()),
            tools_hint: tools_hint.map(|s| s.to_string()),
        }
    }

    fn registry_with_pending(servers: Vec<(&str, McpServerConfig)>) -> ToolRegistry {
        let mut cfg = McpConfig::default();
        for (name, server_cfg) in servers {
            cfg.servers.insert(name.to_string(), server_cfg);
        }
        let mut r = ToolRegistry::new();
        r.load_mcp_lazy(cfg);
        r
    }

    #[test]
    fn no_mcp_connect_when_no_pending_servers() {
        let r = ToolRegistry::new();
        let defs = r.tool_definitions();
        assert!(!defs.iter().any(|d| d["name"] == "mcp_connect"),
            "mcp_connect should not appear when there are no pending servers");
    }

    #[test]
    fn mcp_connect_stub_present_with_pending_servers() {
        let r = registry_with_pending(vec![
            ("fetch", make_cfg(None, None)),
        ]);
        let defs = r.tool_definitions();
        let stub = defs.iter().find(|d| d["name"] == "mcp_connect")
            .expect("mcp_connect stub should be present when servers are pending");
        assert_eq!(stub["input_schema"]["required"][0], "server");
    }

    #[test]
    fn stub_description_includes_server_name() {
        let r = registry_with_pending(vec![
            ("filesystem", make_cfg(None, None)),
        ]);
        let defs = r.tool_definitions();
        let stub = defs.iter().find(|d| d["name"] == "mcp_connect").unwrap();
        let desc = stub["description"].as_str().unwrap();
        assert!(desc.contains("filesystem"), "description should list server name");
    }

    #[test]
    fn stub_description_includes_description_field() {
        let r = registry_with_pending(vec![
            ("fetch", make_cfg(Some("Fetch URLs as markdown"), None)),
        ]);
        let defs = r.tool_definitions();
        let stub = defs.iter().find(|d| d["name"] == "mcp_connect").unwrap();
        let desc = stub["description"].as_str().unwrap();
        assert!(desc.contains("Fetch URLs as markdown"),
            "description field should appear in stub");
    }

    #[test]
    fn stub_description_includes_tools_hint() {
        let r = registry_with_pending(vec![
            ("fs", make_cfg(None, Some("read_file, write_file"))),
        ]);
        let defs = r.tool_definitions();
        let stub = defs.iter().find(|d| d["name"] == "mcp_connect").unwrap();
        let desc = stub["description"].as_str().unwrap();
        assert!(desc.contains("read_file, write_file"),
            "toolsHint should appear in stub description");
    }

    #[test]
    fn stub_lists_multiple_servers_sorted() {
        let r = registry_with_pending(vec![
            ("memory", make_cfg(None, None)),
            ("fetch",  make_cfg(None, None)),
            ("github", make_cfg(None, None)),
        ]);
        let defs = r.tool_definitions();
        let stub = defs.iter().find(|d| d["name"] == "mcp_connect").unwrap();
        let desc = stub["description"].as_str().unwrap();
        // All three names present.
        assert!(desc.contains("fetch") && desc.contains("github") && desc.contains("memory"));
        // Sorted: fetch before github before memory.
        assert!(desc.find("fetch") < desc.find("github"));
        assert!(desc.find("github") < desc.find("memory"));
    }

    #[test]
    fn load_mcp_lazy_stores_all_servers() {
        let r = registry_with_pending(vec![
            ("a", make_cfg(None, None)),
            ("b", make_cfg(None, None)),
        ]);
        assert!(r.has_pending_mcp());
        let pending = r.pending_mcp_servers();
        assert_eq!(pending.len(), 2);
        let names: Vec<&str> = pending.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"a") && names.contains(&"b"));
    }

    #[test]
    fn pending_mcp_servers_returns_description() {
        let r = registry_with_pending(vec![
            ("srv", make_cfg(Some("my description"), None)),
        ]);
        let pending = r.pending_mcp_servers();
        let (_, desc) = pending[0];
        assert_eq!(desc, Some("my description"));
    }
}
