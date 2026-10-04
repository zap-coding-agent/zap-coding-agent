/// Global TUI event channel — send events from any part of the codebase.
///
/// All tui_send() calls are no-ops when not in TUI mode, so they can be
/// added unconditionally to session/stream_highlighter without side-effects.
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc;

// ── Mid-turn "btw" injection queue ────────────────────────────────────────────

/// Messages the user queued via Ctrl+B while a turn was running.
/// Drained by `handle_user_turn` between tool-call rounds.
static BTW_QUEUE: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

pub fn init_btw_queue() {
    BTW_QUEUE.set(Mutex::new(Vec::new())).ok();
}

/// Push a user btw message into the queue (called from TUI event loop).
pub fn push_btw(msg: String) {
    if let Some(mu) = BTW_QUEUE.get() {
        if let Ok(mut q) = mu.lock() {
            q.push(msg);
        }
    }
}

/// Drain all pending btw messages (called from session turn loop).
pub fn drain_btw() -> Vec<String> {
    BTW_QUEUE.get()
        .and_then(|mu| mu.lock().ok())
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

// ── Permission popup (TUI-native) ─────────────────────────────────────────────

/// Sent from `prompt_batch_tui` to the TUI loop; response comes via `response_tx`.
pub struct PermissionPromptRequest {
    pub pending: Vec<(String, String, String)>,
    pub response_tx: tokio::sync::oneshot::Sender<PermissionDecision>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
    Always,
}

static PERM_REQUEST: OnceLock<Mutex<Option<PermissionPromptRequest>>> = OnceLock::new();

pub fn init_perm_channel() {
    PERM_REQUEST.set(Mutex::new(None)).ok();
}

/// Non-blocking — takes the pending request if one exists.
pub fn take_perm_request() -> Option<PermissionPromptRequest> {
    PERM_REQUEST.get().and_then(|mu| mu.lock().ok()).and_then(|mut g| g.take())
}

/// Store a request for the TUI loop to pick up. Returns false if one is already pending.
pub fn set_perm_request(req: PermissionPromptRequest) -> bool {
    if let Some(mu) = PERM_REQUEST.get() {
        if let Ok(mut g) = mu.lock() {
            if g.is_none() {
                *g = Some(req);
                return true;
            }
        }
    }
    false
}

/// Outcome of a finished background agent (`/bg`), reported by the detached
/// tokio task back to the TUI event loop. `Killed` isn't represented here —
/// `/agents kill` sets that status synchronously without going through this event.
#[derive(Debug, Clone)]
pub enum BgOutcome {
    Done { summary: String, files_changed: Vec<String>, turns: usize, tool_calls: usize },
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum TuiEvent {
    LlmChunk(String),
    /// A chunk of extended-thinking text (Anthropic thinking blocks).
    ThinkingChunk(String),
    /// `input` is the tool's raw arguments — the TUI shows only `label`; ACP
    /// mode uses it for file locations and edit diffs.
    ToolStart { id: String, name: String, label: String, input: serde_json::Value },
    ToolDone  { id: String, elapsed_ms: u64, success: bool, preview: String },
    CostUpdate { total_usd: f64, input: u32, output: u32, cache_read: u32 },
    ContextUpdate { pct: u8, turn: usize },
    /// 5-hour/weekly subscription usage window, pushed on every `quota_watch`
    /// check (not just when crossing the warn threshold) so the sidebar can
    /// show a live number. `provider` is a short label ("codex" / "claude").
    QuotaUpdate {
        provider: String,
        five_hour_pct: Option<f32>,
        seven_day_pct: Option<f32>,
        resets_at: Option<String>,
    },
    /// Active skill injected this turn — shown in sidebar, cleared at turn end.
    ActiveSkill(String),
    /// Undelivered btw messages that weren't injected mid-turn (turn ended before next tool call).
    /// The TUI loop auto-submits these as the next user turn so they get a proper response.
    BtwCarryover(Vec<String>),
    /// A standalone notice (hint, warning, status) pushed as a completed assistant bubble.
    /// Use instead of println! in TUI mode so output goes into the chat area, not raw stdout.
    Notice(String),
    /// A red warning banner — used for secret redaction notices and similar.
    Warning(String),
    /// A scheduled job fired — submit `goal` as the next user turn.
    /// `name` is used for display only (shown as the bubble label).
    ScheduledFire { name: String, goal: String },
    /// A `/bg` background agent finished (or failed). `elapsed_secs` is
    /// wall-clock time since it was spawned.
    BackgroundAgentDone { id: String, goal: String, model: String, elapsed_secs: u64, outcome: BgOutcome },
}

static TUI_TX: OnceLock<mpsc::UnboundedSender<TuiEvent>> = OnceLock::new();

pub fn set_tui_sender(tx: mpsc::UnboundedSender<TuiEvent>) {
    let _ = TUI_TX.set(tx);
}

pub fn is_tui_mode() -> bool {
    TUI_TX.get().is_some()
}

pub fn tui_send(event: TuiEvent) {
    if let Some(tx) = TUI_TX.get() {
        let _ = tx.send(event);
    }
}


/// Temporarily suspend TUI raw mode so an inquire/stdin prompt can take over.
/// Safe to call when not in TUI mode (no-op).
pub fn suspend_for_prompt() {
    if is_tui_mode() {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            // Restore default alternate-scroll behavior — see tui/mod.rs.
            crossterm::style::Print("\x1b[?1007h"),
            crossterm::terminal::LeaveAlternateScreen
        );
    }
}

/// Resume TUI raw mode after a prompt completes.
/// The next draw() call will repaint the full screen.
pub fn resume_from_prompt() {
    if is_tui_mode() {
        let _ = crossterm::terminal::enable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::EnterAlternateScreen,
            // No EnableMouseCapture (breaks click-drag copy); disable
            // alternate scroll mode too — see tui/mod.rs.
            crossterm::style::Print("\x1b[?1007l"),
        );
    }
}
