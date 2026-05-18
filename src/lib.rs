pub mod agent_core;
pub mod audit;
pub mod cli;
pub mod code_index;
pub mod config;
pub mod context_manager;
pub mod llm_client;
pub mod mcp;
pub mod permission_manager;
pub mod persistence;
pub mod secret_scanner;
pub mod session;
pub mod shell_runner;
pub mod skill_manager;
pub mod snapshot;
pub mod stream_highlighter;
pub mod task_planner;
pub mod tools;
pub mod ui;
pub mod workflow;

use anyhow::Result;

pub async fn run() -> Result<()> {
    cli::run().await
}
