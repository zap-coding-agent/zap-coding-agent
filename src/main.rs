use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    // Enable ANSI escape code processing in Windows CMD / PowerShell.
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_string()),
        )
        .init();
    agent_harness::run().await
}
