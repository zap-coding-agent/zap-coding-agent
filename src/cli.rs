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
  /provider  Switch provider interactively\n\
  /sessions  Browse & resume old sessions\n\
  /clear     Clear conversation history\n\
  /exit      Quit")]
pub struct Args {
    /// Goal to execute (omit for interactive REPL)
    #[arg(long)]
    pub goal: Option<String>,

    /// Output format for single-shot mode: text (default) or json
    #[arg(long, default_value = "text")]
    pub output_format: String,

    /// Auto-approve all tool operations — shorthand for AGENT_PERMISSION_MODE=auto.
    /// Ideal for CI pipelines and scripts. Implies non-interactive mode.
    #[arg(long, short = 'y')]
    pub auto: bool,

    /// Token budget: warn at 80% of N tokens and refuse new turns at 100%.
    /// Overrides the model's default context window for budget tracking.
    /// Example: --budget 50000
    #[arg(long)]
    pub budget: Option<u32>,

    /// SDK / headless mode: read newline-delimited prompts from stdin, run each as
    /// a conversation turn, output the agent's text reply to stdout as JSON.
    /// Implies --auto. Useful for scripting, remote control, and GitLab CI.
    ///
    /// Protocol — stdin (one JSON object per line):
    ///   {"type":"user","text":"your prompt here"}
    ///   {"type":"quit"}
    ///
    /// Protocol — stdout (one JSON object per line):
    ///   {"type":"assistant","text":"...","turn":N,"ctx_pct":N}
    ///   {"type":"error","message":"..."}
    #[arg(long)]
    pub sdk: bool,

    /// Use the classic REPL mode instead of the TUI (default).
    /// Also set by AGENT_NO_TUI=1.
    #[arg(long)]
    pub cli: bool,
}

// ── Banner ────────────────────────────────────────────────────────────────────
//
// Box geometry (all measurements in displayed columns):
//   Total width      = 72
//   Inner width (BW) = 68   (between the │ walls, after the 2-char indent)
//   Left col (LC)    = 36   (content chars in left panel of two-col rows)
//   Right col (RC)   = 27   (content chars in right panel)
//   " │ " separator  =  3
//   " " left + " │" right = 2+2 = 4  → 36+3+27+2 = 68 ✓  (actually 4+36+3+27+2=72 total ✓)

const BW: usize = 68;
const LC: usize = 36;
const RC: usize = 27;

fn box_top() -> String {
    format!("  ╭{}╮", "─".repeat(BW).truecolor(60, 55, 80))
}
fn box_bot() -> String {
    format!("  ╰{}╯", "─".repeat(BW).truecolor(60, 55, 80))
}
fn box_div() -> String {
    format!(
        "  {}{}{}{}{}",
        "├".truecolor(60,55,80),
        "─".repeat(LC + 2).truecolor(60,55,80),
        "┬".truecolor(60,55,80),
        "─".repeat(RC + 2).truecolor(60,55,80),
        "┤".truecolor(60,55,80),
    )
}
fn box_empty() -> String {
    format!("  {}{}{}",
        "│".truecolor(60,55,80), " ".repeat(BW), "│".truecolor(60,55,80))
}

/// Full-width row — raw for width math, fmt for display.
#[allow(dead_code)]
fn box_fw(raw: &str, fmt: &str) -> String {
    let content_w = BW - 2; // 66: 1 space each side
    let pad = content_w.saturating_sub(raw.len());
    format!("  {} {}{} {}",
        "│".truecolor(60,55,80), fmt, " ".repeat(pad), "│".truecolor(60,55,80))
}

/// Two-column row.
/// `l_raw` / `r_raw` are plain-text versions used only for display-width math.
/// `.chars().count()` is used so multi-byte Unicode (…, ↑↓, etc.) counts correctly.
fn box_tc(l_raw: &str, l_fmt: &str, r_raw: &str, r_fmt: &str) -> String {
    let l_pad = LC.saturating_sub(l_raw.chars().count());
    let r_pad = RC.saturating_sub(r_raw.chars().count());
    format!("  {} {}{} {} {}{} {}",
        "│".truecolor(60,55,80),
        l_fmt, " ".repeat(l_pad),
        "│".truecolor(60,55,80),
        r_fmt, " ".repeat(r_pad),
        "│".truecolor(60,55,80),
    )
}

/// Centered full-width art row (raw len provided explicitly for alignment).
fn box_art(_raw: &str, fmt: &str, raw_len: usize) -> String {
    let content_w = BW - 2; // 66
    let pad_l = (content_w.saturating_sub(raw_len)) / 2;
    let pad_r = content_w.saturating_sub(raw_len).saturating_sub(pad_l);
    format!("  {} {}{}{} {}",
        "│".truecolor(60,55,80),
        " ".repeat(pad_l), fmt, " ".repeat(pad_r),
        "│".truecolor(60,55,80))
}

pub fn print_banner(config: &crate::config::Config) {
    use crate::config::{Provider, PermissionMode};

    let ver = format!("v{}", VERSION);

    // ── derive display strings ────────────────────────────────────────────────
    // Truncate at 25 display chars so {:<10} label + " " + value = max 36 = LC.
    let server_raw = match &config.provider {
        Provider::Anthropic => "Anthropic API".to_string(),
        Provider::OpenAi    => config.base_url.as_deref()
            .map(|u| {
                // Strip endpoint suffix so we display just the host/base path.
                let u = u.strip_suffix("/chat/completions").unwrap_or(u);
                let u = u.strip_suffix("/v1").unwrap_or(u);
                u.trim_start_matches("http://").trim_start_matches("https://").to_string()
            })
            .unwrap_or_else(|| "OpenAI API".to_string()),
    };
    let server_raw = if server_raw.chars().count() > 25 {
        format!("{}…", server_raw.chars().take(24).collect::<String>())
    } else { server_raw };
    let model_raw = if config.model.chars().count() > 25 {
        format!("{}…", config.model.chars().take(24).collect::<String>())
    } else { config.model.clone() };
    let mode_raw = match config.permission_mode {
        PermissionMode::Ask  => "ask",
        PermissionMode::Auto => "auto",
        PermissionMode::Deny => "deny",
    };

    // ── ASCII art "ZAP" — 4 lines × 21 display cols ───────────────────────────
    //
    //  Each letter is 8 cols wide (right-padded), P is 5 cols: 8+8+5 = 21
    //
    //  Z (cols 0-7):   top bar ─────  diagonal  ────────  bottom bar
    //   _____    _     ___
    //    ___/   /_\   | _ \
    //   /      / _ \  |  _/
    //  /_____  /_/ \_\ |_|
    //
    let art_lines: &[&str] = &[
        " _____     _     ___ ",        // Z-top    A-top    P-top
        "  ___/    /_\\   | _ \\",      // Z-diag   A-sides  P-curve
        " /       / _ \\  |  _/",       // Z-mid    A-cross  P-close
        "/_____  /_/ \\_\\ |_|  ",      // Z-base   A-base   P-stem
    ];
    let art_colors: &[(u8, u8, u8)] = &[
        (255, 215, 40),
        (255, 190, 10),
        (255, 165,  0),
        (255, 140,  0),
    ];

    // ── subtitle ──────────────────────────────────────────────────────────────
    let sub_raw = format!("fast AI coding agent  {}", ver);
    let sub_len = sub_raw.chars().count();
    let sub_fmt = format!("{}  {}",
        "fast AI coding agent".truecolor(120, 115, 145),
        ver.truecolor(100, 210, 255).bold());

    // ── cwd (trimmed, home replaced with ~) ───────────────────────────────────
    let cwd = std::env::current_dir()
        .ok()
        .and_then(|p| {
            let home = std::env::var("HOME").ok()?;
            let s = p.to_string_lossy().into_owned();
            Some(if s.starts_with(&home) { s.replacen(&home, "~", 1) } else { s })
        })
        .unwrap_or_else(|| ".".to_string());
    let cwd_disp: String = if cwd.chars().count() > 34 {
        format!("…{}", cwd.chars().rev().take(33).collect::<String>().chars().rev().collect::<String>())
    } else { cwd };

    // ── print box ─────────────────────────────────────────────────────────────
    println!();
    println!("{}", box_top());
    println!("{}", box_empty());

    // ASCII art ZAP — centered (21 display cols in 66-col content area)
    for (i, raw) in art_lines.iter().enumerate() {
        let (r, g, b) = art_colors[i];
        // raw contains Rust escape sequences; actual display len = 21
        let fmt = raw.truecolor(r, g, b).bold().to_string();
        println!("{}", box_art(raw, &fmt, 21));
    }

    println!("{}", box_empty());
    println!("{}", box_art(&sub_raw, &sub_fmt, sub_len));
    println!("{}", box_empty());
    println!("{}", box_div());

    // ── two-column rows: left = config/cwd  right = tips ──────────────────────
    // LC=36: {:<10}(10) + " "(1) + value(≤25) = max 36 ✓
    // RC=27: all r_raw strings ≤27 display cols; box_tc now uses .chars().count()

    // row 0: model | Tips header
    let l0_raw = format!("{:<10} {}", "model", model_raw);
    let l0_fmt = format!("{:<10} {}", "model".truecolor(140,130,160), model_raw.truecolor(100,210,255).bold());
    let r0_raw = "Tips for getting started";                             // 24 cols
    let r0_fmt = r0_raw.truecolor(255, 185, 0).bold().to_string();
    println!("{}", box_tc(&l0_raw, &l0_fmt, r0_raw, &r0_fmt));

    // row 1: backend | Tab ↑↓  (↑↓ now counted correctly via .chars().count())
    let l1_raw = format!("{:<10} {}", "backend", server_raw);
    let l1_fmt = format!("{:<10} {}", "backend".truecolor(140,130,160), server_raw.truecolor(100,210,255));
    let r1_raw = "  Tab  ↑↓  autocomplete";                             // 23 cols
    let r1_fmt = format!("  {}  {}  {}",
        "Tab".truecolor(100,210,255).bold(),
        "↑↓".truecolor(100,210,255).bold(),
        "autocomplete".truecolor(110,105,130));
    println!("{}", box_tc(&l1_raw, &l1_fmt, r1_raw, &r1_fmt));

    // row 2: mode | /  commands
    let l2_raw = format!("{:<10} {}", "mode", mode_raw);
    let l2_fmt = format!("{:<10} {}", "mode".truecolor(140,130,160), mode_raw.truecolor(100,210,255));
    let r2_raw = "  /          commands";                               // 21 cols
    let r2_fmt = format!("  {}          {}",
        "/".truecolor(100,210,255).bold(),
        "commands".truecolor(110,105,130));
    println!("{}", box_tc(&l2_raw, &l2_fmt, r2_raw, &r2_fmt));

    // row 3: (empty) | /provider
    let r3_raw = "  /provider  switch LLM";                            // 23 cols
    let r3_fmt = format!("  {}  {}",
        "/provider".truecolor(100,210,255).bold(),
        "switch LLM".truecolor(110,105,130));
    println!("{}", box_tc("", "", r3_raw, &r3_fmt));

    // row 4: cwd | /help
    let r4_raw = "  /help      all commands";                          // 25 cols
    let r4_fmt = format!("  {}      {}",
        "/help".truecolor(100,210,255).bold(),
        "all commands".truecolor(110,105,130));
    println!("{}", box_tc(&cwd_disp, &cwd_disp.truecolor(110,105,130).dimmed().to_string(), r4_raw, &r4_fmt));

    println!("{}", box_bot());
    println!();
}

pub async fn run() -> Result<()> {
    let args = Args::parse();
    let mut config = crate::config::Config::load()?;

    if args.output_format.eq_ignore_ascii_case("json") {
        config.output_format = crate::config::OutputFormat::Json;
    }

    // --auto / --sdk both imply auto permission mode.
    if args.auto || args.sdk {
        config.permission_mode = crate::config::PermissionMode::Auto;
    }

    if let Some(b) = args.budget {
        config.budget = Some(b);
    }

    if args.sdk {
        tracing::info!(model = %config.model, "SDK mode");
        return crate::agent_core::run_sdk(&config).await;
    }

    match args.goal {
        Some(goal) => {
            tracing::info!(goal = %goal, model = %config.model, "single-shot mode");
            crate::agent_core::run(&goal, &config).await
        }
        None => {
            let use_tui = !args.cli && std::env::var("AGENT_NO_TUI").is_err();
            if use_tui {
                tracing::info!(model = %config.model, "TUI mode");
                crate::agent_core::run_tui(&config).await
            } else {
                print_banner(&config);
                let _ = <std::io::Stdout as std::io::Write>::flush(&mut std::io::stdout());
                tracing::info!(model = %config.model, "interactive REPL mode");
                crate::agent_core::run_repl(&config).await
            }
        }
    }
}
