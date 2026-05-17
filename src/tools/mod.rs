/// Tool trait, registry, and MCP integration. Tool implementations live in submodules.
use anyhow::Result;
use async_trait::async_trait;
use colored::Colorize;
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
}

// ── Registry ──────────────────────────────────────────────────────────────────

pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        use file::{BatchEditTool, EditFileTool, GlobReadTool, ReadFileTool, UndoEditTool, WriteFileTool};
        use search::{CodeMapTool, FindDefinitionTool, FindReferencesTool, SearchCodeTool};
        use shell::{GitStatusTool, ListDirectoryTool, ShellTool};
        use web::{WebFetchTool, WebSearchTool};

        let mut r = Self { tools: HashMap::new() };
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

    pub async fn register_mcp_servers(&mut self) {
        let cfg = crate::mcp::load_config();
        if cfg.servers.is_empty() { return; }

        for (server_name, server_cfg) in &cfg.servers {
            match crate::mcp::McpServer::connect(server_cfg).await {
                Ok((server, tools)) => {
                    let srv = std::sync::Arc::new(tokio::sync::Mutex::new(server));
                    for tool_def in tools {
                        let mcp_tool = crate::mcp::McpTool {
                            name: tool_def.name.clone(),
                            description: tool_def.description.unwrap_or_default(),
                            schema: tool_def.input_schema,
                            server: srv.clone(),
                        };
                        tracing::info!(server = %server_name, tool = %tool_def.name, "registered MCP tool");
                        self.register(std::sync::Arc::new(mcp_tool));
                    }
                }
                Err(e) => {
                    tracing::warn!(server = %server_name, "MCP server failed to start: {}", e);
                    eprintln!("  {} MCP server '{}' unavailable: {}", "⚠".yellow(), server_name, e);
                }
            }
        }
    }

    pub fn tool_definitions(&self) -> Vec<serde_json::Value> {
        let mut defs: Vec<serde_json::Value> = self
            .tools
            .values()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "input_schema": t.input_schema(),
                })
            })
            .collect();
        defs.sort_by_key(|d| d["name"].as_str().unwrap_or("").to_string());
        defs
    }
}
