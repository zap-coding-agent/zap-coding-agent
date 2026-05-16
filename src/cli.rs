use anyhow::Result;
use clap::Parser;
use colored::Colorize;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser, Debug)]
#[command(name = "zap")]
#[command(about = "⚡ zap — fast AI coding agent")]
#[command(long_about = "\
⚡ zap — fast AI coding agent\n\
\n\
Modes:\n\
  zap                  Interactive REPL (multi-turn)\n\
  zap --goal \"...\"    Single-shot: run one goal and exit\n\
\n\
Slash commands (REPL only):\n\
  /help      Show this help\n\
  /config    Show current provider, model, URL\n\
  /models    List models available on the server\n\
  /model X   Switch to model X for this session\n\
  /clear     Clear conversation history\n\
  /history   Show turn count\n\
  /exit      Quit")]
pub struct Args {
    /// Goal to execute (omit for interactive REPL)
    #[arg(long)]
    pub goal: Option<String>,
}

pub fn print_banner(config: &crate::config::Config) {
    use crate::config::Provider;

    let provider_label = match config.provider {
        Provider::Anthropic => "Anthropic".to_string(),
        Provider::OpenAi => config.base_url.as_deref()
            .map(|u| {
                // Strip http(s):// for display
                let u = u.trim_start_matches("http://").trim_start_matches("https://");
                format!("LM Studio @ {}", u)
            })
            .unwrap_or_else(|| "OpenAI".to_string()),
    };

    println!();
    println!(
        "  {}  {}",
        "⚡ zap".bright_yellow().bold(),
        format!("v{}", VERSION).dimmed()
    );
    println!("  {}", "─────────────────────────────────────────".dimmed());
    println!(
        "  {}  {}    {}  {}",
        "model".dimmed(), config.model.cyan().bold(),
        "·".dimmed(), provider_label.dimmed()
    );
    println!(
        "  {}",
        "Tab = autocomplete  ↑↓ = history  /help = commands".dimmed()
    );
    println!();
}

pub async fn run() -> Result<()> {
    let args = Args::parse();
    let config = crate::config::Config::load()?;

    match args.goal {
        Some(goal) => {
            tracing::info!(goal = %goal, model = %config.model, "single-shot mode");
            crate::agent_core::run(&goal, &config).await
        }
        None => {
            print_banner(&config);
            tracing::info!(model = %config.model, "interactive REPL mode");
            crate::agent_core::run_repl(&config).await
        }
    }
}
