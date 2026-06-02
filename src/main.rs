use anyhow::Result;
use std::io::Write as _;

#[tokio::main]
async fn main() -> Result<()> {
    // Enable ANSI escape code processing in Windows CMD / PowerShell.
    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).ok();

    // Write tracing to stderr so it doesn't corrupt the TUI alternate screen.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_string()),
        )
        .init();

    // Panic hook: restore terminal (in case TUI was active) and write the
    // panic info to ~/.zap/zap.log before dying, so crashes aren't silent.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stderr(),
            crossterm::terminal::LeaveAlternateScreen
        );
        let log = zap_coding_agent::log::log_path();
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&log) {
            let _ = writeln!(f, "[PANIC] {info}");
        }
        prev_hook(info);
    }));

    zap_coding_agent::run().await
}
