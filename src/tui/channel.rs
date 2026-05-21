/// Global TUI event channel — send events from any part of the codebase.
///
/// All tui_send() calls are no-ops when not in TUI mode, so they can be
/// added unconditionally to session/stream_highlighter without side-effects.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tokio::sync::mpsc;

/// Set while `prompt_batch_tui` owns the crossterm event queue so the TUI
/// tick loop skips its own `event::poll` and doesn't steal Y/N/A keypresses.
static PERM_PROMPT_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn enter_permission_prompt() {
    PERM_PROMPT_ACTIVE.store(true, Ordering::SeqCst);
}

pub fn exit_permission_prompt() {
    PERM_PROMPT_ACTIVE.store(false, Ordering::SeqCst);
}

pub fn is_permission_prompt_active() -> bool {
    PERM_PROMPT_ACTIVE.load(Ordering::SeqCst)
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
