pub mod agent_core;
pub mod audit;
pub mod cli;
pub mod config;
pub mod context_manager;
pub mod llm_client;
pub mod permission_manager;
pub mod persistence;
pub mod shell_runner;
pub mod tool_registry;

use anyhow::Result;

pub async fn run() -> Result<()> {
    cli::run().await
}
