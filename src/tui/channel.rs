/// Global TUI event channel — send events from any part of the codebase.
///
/// All tui_send() calls are no-ops when not in TUI mode, so they can be
/// added unconditionally to session/stream_highlighter without side-effects.
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc;

// ── Permission popup (TUI-native) ─────────────────────────────────────────────

/// Sent from `prompt_batch_tui` to the TUI loop; response comes via `response_tx`.
pub struct PermissionPromptRequest {
    pub pending: Vec<(String, String, String)>,
    pub response_tx: std::sync::mpsc::SyncSender<PermissionDecision>,
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

#[derive(Debug, Clone)]
pub enum TuiEvent {
    LlmChunk(String),
    /// A chunk of extended-thinking text (Anthropic thinking blocks).
    ThinkingChunk(String),
    ToolStart { id: String, name: String, label: String },
    ToolDone  { id: String, elapsed_ms: u64, success: bool, preview: String },
    CostUpdate { total_usd: f64, input: u32, output: u32, cache_read: u32 },
    ContextUpdate { pct: u8, turn: usize },
    /// Active skill injected this turn — shown in sidebar, cleared at turn end.
    ActiveSkill(String),
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
            crossterm::terminal::EnterAlternateScreen
        );
    }
}
