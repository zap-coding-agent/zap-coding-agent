/// Terminal UI primitives: spinner, REPL helper, command picker, cost formatting.
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use rustyline::{
    Cmd, ConditionalEventHandler, RepeatCount,
    completion::{Completer, Pair},
    highlight::Highlighter,
    hint::{Hint, Hinter},
    validate::Validator,
    Context, Helper,
};
use std::borrow::Cow;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::llm_client::Usage;

// ── Cost helpers ──────────────────────────────────────────────────────────────

/// USD per million tokens for known Claude model families. Returns (0, 0) for
/// local/unknown models so the caller can skip cost display.
pub fn cost_per_million(model: &str) -> (f64, f64) {
    if model.contains("opus-4") {
        (15.0, 75.0)
    } else if model.contains("sonnet-4") {
        (3.0, 15.0)
    } else if model.contains("haiku-4") {
        (0.8, 4.0)
    } else {
        (0.0, 0.0)
    }
}

pub fn format_cost(usage: &Usage, model: &str) -> String {
    let (cost_in, cost_out) = cost_per_million(model);
    let input_k  = usage.input_tokens  as f64 / 1_000.0;
    let output_k = usage.output_tokens as f64 / 1_000.0;
    let total_usd = (usage.input_tokens  as f64 * cost_in
                   + usage.output_tokens as f64 * cost_out)
                   / 1_000_000.0;

    let cache_note = if usage.cache_read_tokens > 0 {
        format!(
            "  {}  cache hit {}k / write {}k",
            "●".bright_blue(),
            usage.cache_read_tokens  / 1000,
            usage.cache_write_tokens / 1000,
        )
    } else {
        String::new()
    };

    if cost_in > 0.0 {
        format!("{}k in / {}k out  (~${:.4}){}", input_k as u32, output_k as u32, total_usd, cache_note)
    } else {
        format!("{}k in / {}k out{}", input_k as u32, output_k as u32, cache_note)
    }
}

// ── Tool icon ─────────────────────────────────────────────────────────────────

pub fn tool_icon(name: &str) -> &'static str {
    match name {
        "read_file" | "list_directory"                  => "▸",
        "write_file" | "edit_file" | "batch_edit"       => "✦",
        "shell"                                         => "»",
        "search_code" | "find_definition" | "code_map"  => "⊙",
        n if n.starts_with("git_")                      => "⎇",
        n if n.starts_with("memory_")                   => "◇",
        "snapshot_restore" | "snapshot_list"            => "◈",
        _                                               => "◆",
    }
}

// ── Thinking spinner ──────────────────────────────────────────────────────────

static THINKING_PHRASES: &[&str] = &[
    "Thinking…", "Pondering…", "Considering…", "Percolating…", "Analyzing…",
    "Reasoning…", "Reflecting…", "Contemplating…", "Deliberating…", "Evaluating…",
    "Processing…", "Computing…", "Calculating…", "Synthesizing…", "Formulating…",
    "Composing…", "Drafting…", "Planning…", "Strategizing…", "Investigating…",
    "Exploring…", "Examining…", "Inspecting…", "Reviewing…", "Scanning…",
    "Searching…", "Checking…", "Verifying…", "Assessing…", "Inferring…",
    "Deducing…", "Hypothesizing…", "Brainstorming…", "Generating…", "Constructing…",
    "Organizing…", "Architecting…", "Designing…", "Navigating…", "Mapping…",
    "Iterating…", "Crunching…", "Wrangling…", "Optimizing…", "Refining…",
    "Calibrating…", "Tuning…", "Adapting…", "Learning…", "Absorbing…",
    "Digesting…", "Integrating…", "Distilling…", "Clarifying…", "Resolving…",
    "Noodling…", "Cogitating…", "Ruminating…", "Musing…", "Meditating…",
    "Mulling…", "Stewing…", "Simmering…", "Perusing…", "Untangling…",
    "Deciphering…", "Parsing…", "Unpacking…", "Grokking…", "Connecting dots…",
    "Following threads…", "Tracing paths…", "Gathering thoughts…", "Firing neurons…",
    "Spinning up…", "Charting a course…", "Consulting the scrolls…",
    "Extrapolating…", "Interpolating…", "Reconciling…", "Cross-referencing…",
    "Pattern-matching…", "Decompiling…", "Indexing…", "Vectorizing…",
    "Triangulating…", "Approximating…", "Reticulating splines…", "Compiling…",
    "Bootstrapping…", "Initializing…", "Assembling…", "Recalibrating…",
    "Deep diving…", "Zooming out…", "Zooming in…", "Reading the room…",
    "Channeling wisdom…", "Having thoughts…", "Making connections…",
];

pub struct ThinkingSpinner {
    pb:      ProgressBar,
    stop:    Arc<AtomicBool>,
    /// Set by the thread just before it exits — lets before_output wait for
    /// the thread to fully stop before clearing the bar and printing output.
    /// This eliminates the race between indicatif redraws and streaming text.
    stopped: Arc<AtomicBool>,
    thread:  Option<std::thread::JoinHandle<()>>,
}

impl Default for ThinkingSpinner {
    fn default() -> Self { Self::new() }
}

impl ThinkingSpinner {
    pub fn new() -> Self {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::with_template("  {spinner:.yellow} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        // Do NOT call enable_steady_tick — its internal thread can fire after
        // finish_and_clear() and overwrite streaming text on Windows. We tick
        // manually from our own thread so we control exactly when it stops.

        let stop    = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let stop_clone    = stop.clone();
        let stopped_clone = stopped.clone();
        let pb_clone = pb.clone();

        let start_idx = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as usize)
            .unwrap_or(0);

        let thread = std::thread::spawn(move || {
            let mut i = start_idx;
            let mut ticks: usize = 0;
            pb_clone.set_message(THINKING_PHRASES[i % THINKING_PHRASES.len()]);
            loop {
                if stop_clone.load(Ordering::Acquire) {
                    stopped_clone.store(true, Ordering::Release);
                    return;
                }
                pb_clone.tick();
                std::thread::sleep(std::time::Duration::from_millis(80));
                ticks += 1;
                if ticks.is_multiple_of(25) {
                    i += 1;
                    if !stop_clone.load(Ordering::Acquire) {
                        pb_clone.set_message(THINKING_PHRASES[i % THINKING_PHRASES.len()]);
                    }
                }
            }
        });

        Self { pb, stop, stopped, thread: Some(thread) }
    }

    pub fn pb_clone(&self) -> ProgressBar { self.pb.clone() }
    pub fn stop_signal(&self) -> Arc<AtomicBool> { self.stop.clone() }
    pub fn stopped_signal(&self) -> Arc<AtomicBool> { self.stopped.clone() }

    /// A no-op spinner used in TUI mode where the TUI event loop handles animation.
    /// No thread is spawned; all methods are safe no-ops.
    pub fn noop() -> Self {
        Self {
            pb:      ProgressBar::hidden(),
            stop:    Arc::new(AtomicBool::new(true)),
            stopped: Arc::new(AtomicBool::new(true)),
            thread:  None,
        }
    }

    pub fn finish_and_clear(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
        self.pb.finish_and_clear();
    }
}

// ── Theme ─────────────────────────────────────────────────────────────────────

/// Named colour palette — prevents 30+ scattered `truecolor()` calls from drifting.
pub mod theme {
    pub const PRIMARY:  (u8, u8, u8) = (255, 210,  50); // gold   — headings, accents
    pub const MUTED:    (u8, u8, u8) = (100,  95, 130); // muted  — labels, hints
    pub const BORDER:   (u8, u8, u8) = ( 60,  55,  80); // border — horizontal rules
    pub const ACCENT:   (u8, u8, u8) = (100, 210, 255); // cyan   — values, links
    pub const BOX:      (u8, u8, u8) = ( 70,  65,  90); // dark   — ╭─ ╰─ box drawing
    pub const SKILL:    (u8, u8, u8) = (255, 200,  60); // amber  — skill labels
    pub const SUB:      (u8, u8, u8) = (120, 115, 140); // grey   — descriptions
    pub const INFO:     (u8, u8, u8) = (150, 200, 150); // green  — hook / info output
    pub const WARN:     (u8, u8, u8) = (180, 100,  80); // orange — warnings
}

/// Standard inquire picker style used across all slash-command pickers.
pub fn inquire_render_config() -> inquire::ui::RenderConfig<'static> {
    use inquire::ui::{Attributes, Color, RenderConfig, StyleSheet, Styled};
    RenderConfig::default()
        .with_prompt_prefix(Styled::new("  ◆").with_fg(Color::LightYellow))
        .with_highlighted_option_prefix(Styled::new(" ❯").with_fg(Color::LightYellow))
        .with_selected_option(Some(
            StyleSheet::new().with_fg(Color::LightCyan).with_attr(Attributes::BOLD),
        ))
        .with_help_message(StyleSheet::new().with_fg(Color::DarkGrey))
}

// ── Slash command table ───────────────────────────────────────────────────────

pub const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/help",        "show commands"),
    ("/config",      "show provider, model, URL"),
    ("/models",      "list models on server"),
    ("/model",       "<id>  switch model"),
    ("/permissions", "ask|auto|deny"),
    ("/clear",       "clear history"),
    ("/compact",     "compress conversation"),
    ("/init",        "set up project (ZAP.md, index, project.json)"),
    ("/attach",      "<path>  stage image file"),
    ("/paste",       "paste image from clipboard"),
    ("/history",     "show turn count"),
    ("/sessions",    "pick & resume old session"),
    ("/provider",    "switch LM Studio / DeepSeek / Anthropic"),
    ("/memory",      "list|get|set|del"),
    ("/audit",       "[N]  audit log"),
    ("/hooks",       "list configured hooks"),
    ("/mcp",         "list|edit|edit project|path  MCP servers"),
    ("/tasks",       "browse & execute task sessions"),
    ("/index",       "[path|stats]  reindex AST code symbols"),
    ("/undo",        "[file]  undo last file edit"),
    ("/cost",        "token usage & cost"),
    ("/run",         "<name>  run a workflow"),
    ("/workflow",    "new <name>  scaffold a workflow"),
    ("/skill",       "list|show|create|capture"),
    ("/branch",      "<name>  fork conversation"),
    ("/branches",    "list conversation branches"),
    ("/switch",      "<name>  switch branch"),
    ("/exit",        "quit"),
];

// ── Interactive command picker ────────────────────────────────────────────────

pub fn show_command_picker() -> Option<String> {
    use inquire::{ui::{Attributes, Color, RenderConfig, StyleSheet}, Select};

    let cfg = RenderConfig::default()
        .with_prompt_prefix(inquire::ui::Styled::new("  ⚡").with_fg(Color::LightYellow))
        .with_highlighted_option_prefix(inquire::ui::Styled::new(" ❯").with_fg(Color::LightYellow))
        .with_selected_option(Some(
            StyleSheet::new().with_fg(Color::LightCyan).with_attr(Attributes::BOLD),
        ))
        .with_help_message(StyleSheet::new().with_fg(Color::DarkGrey));

    let entries: Vec<String> = SLASH_COMMANDS
        .iter()
        .map(|(cmd, desc)| format!("  {:<24}{}", cmd, desc))
        .collect();

    match Select::new("Commands", entries)
        .with_render_config(cfg)
        .with_help_message("↑↓  navigate    type to filter    Enter  select    Esc  cancel")
        .with_page_size(18)
        .prompt_skippable()
    {
        Ok(Some(line)) => line.split_whitespace().next().map(str::to_string),
        _ => None,
    }
}

// ── '/' key binding: open picker immediately on empty buffer ──────────────────

pub struct SlashHandler {
    pub triggered: Arc<Mutex<bool>>,
}

impl ConditionalEventHandler for SlashHandler {
    fn handle(
        &self,
        _evt: &rustyline::Event,
        _n: RepeatCount,
        _positive: bool,
        ctx: &rustyline::EventContext<'_>,
    ) -> Option<Cmd> {
        if ctx.line().is_empty() {
            *self.triggered.lock().unwrap() = true;
            Some(Cmd::AcceptLine)
        } else {
            None
        }
    }
}

// ── Rustyline helper: completion, hints, highlighting ─────────────────────────

#[derive(Clone)]
pub struct CommandHint(pub String);
impl Hint for CommandHint {
    fn display(&self) -> &str { &self.0 }
    fn completion(&self) -> Option<&str> { Some(&self.0) }
}

pub struct ZapHelper;
impl Helper   for ZapHelper {}
impl Validator for ZapHelper {}

impl Hinter for ZapHelper {
    type Hint = CommandHint;

    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<CommandHint> {
        if !line.starts_with('/') || pos != line.len() || line.contains(' ') {
            return None;
        }
        SLASH_COMMANDS.iter()
            .find(|(cmd, _)| cmd.starts_with(line))
            .map(|(cmd, desc)| CommandHint(format!("{}  {}", &cmd[line.len()..], desc)))
    }
}

impl Highlighter for ZapHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if line.starts_with('/') { Cow::Owned(line.cyan().to_string()) }
        else                     { Cow::Borrowed(line) }
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(hint.dimmed().to_string())
    }
    fn highlight_char(&self, line: &str, _pos: usize, _forced: bool) -> bool {
        line.starts_with('/')
    }
}

impl Completer for ZapHelper {
    type Candidate = Pair;

    fn complete(&self, line: &str, pos: usize, _ctx: &Context<'_>)
        -> rustyline::Result<(usize, Vec<Pair>)>
    {
        if !line.starts_with('/') { return Ok((0, vec![])); }
        let prefix = &line[..pos];
        let matches = SLASH_COMMANDS.iter()
            .filter(|(cmd, _)| cmd.starts_with(prefix))
            .map(|(cmd, desc)| Pair {
                display:     format!("{:<20} {}", cmd, desc),
                replacement: format!("{} ", cmd),
            })
            .collect();
        Ok((0, matches))
    }
}
