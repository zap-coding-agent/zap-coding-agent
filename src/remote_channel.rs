/// Global channels for zap remote control.
///
/// Activated once by `/remote`. After that, any part of the codebase can call
/// `send_chunk` / `send_done` without caring whether remote is active.
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tokio::sync::{broadcast, mpsc};
use tokio::task::AbortHandle;

static ACTIVE: AtomicBool = AtomicBool::new(false);
static CHUNK_TX: OnceLock<broadcast::Sender<String>> = OnceLock::new();
static DONE_TX:  OnceLock<broadcast::Sender<()>>     = OnceLock::new();
static INPUT_TX: OnceLock<mpsc::UnboundedSender<String>>          = OnceLock::new();
static INPUT_RX: OnceLock<Mutex<mpsc::UnboundedReceiver<String>>> = OnceLock::new();
static SERVER_ABORT: OnceLock<Mutex<Option<AbortHandle>>>         = OnceLock::new();
static TUNNEL_PID: AtomicU32 = AtomicU32::new(0);

/// Set up channels. Idempotent — safe to call again after `deactivate`.
pub fn activate() {
    if CHUNK_TX.get().is_none() {
        let (chunk_tx, _) = broadcast::channel(512);
        let (done_tx, _)  = broadcast::channel(16);
        let (input_tx, input_rx) = mpsc::unbounded_channel();
        let _ = CHUNK_TX.set(chunk_tx);
        let _ = DONE_TX.set(done_tx);
        let _ = INPUT_TX.set(input_tx);
        let _ = INPUT_RX.set(Mutex::new(input_rx));
        let _ = SERVER_ABORT.set(Mutex::new(None));
    }
    ACTIVE.store(true, Ordering::SeqCst);
}

/// Stop the HTTP server and kill the tunnel process.
pub fn deactivate() {
    ACTIVE.store(false, Ordering::SeqCst);
    if let Some(lock) = SERVER_ABORT.get() {
        if let Ok(mut g) = lock.lock() {
            if let Some(h) = g.take() { h.abort(); }
        }
    }
    let pid = TUNNEL_PID.swap(0, Ordering::SeqCst);
    if pid > 0 { kill_pid(pid); }
}

fn kill_pid(pid: u32) {
    #[cfg(unix)]
    { let _ = std::process::Command::new("kill").arg(pid.to_string()).spawn(); }
    #[cfg(windows)]
    { let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/F"]).spawn(); }
}

pub fn set_server_abort(h: AbortHandle) {
    if let Some(lock) = SERVER_ABORT.get() {
        if let Ok(mut g) = lock.lock() { *g = Some(h); }
    }
}

pub fn set_tunnel_pid(pid: u32) {
    TUNNEL_PID.store(pid, Ordering::SeqCst);
}

pub fn is_active() -> bool { ACTIVE.load(Ordering::SeqCst) }

/// Called from the LLM streaming loop for every text chunk.
pub fn send_chunk(text: &str) {
    if let Some(tx) = CHUNK_TX.get() { let _ = tx.send(text.to_string()); }
}

/// Called once after each full agent turn completes.
pub fn send_done() {
    if let Some(tx) = DONE_TX.get() { let _ = tx.send(()); }
}

/// Subscribe to (chunks, done) — one pair per WebSocket connection.
pub fn subscribe() -> Option<(broadcast::Receiver<String>, broadcast::Receiver<()>)> {
    let chunk_rx = CHUNK_TX.get()?.subscribe();
    let done_rx  = DONE_TX.get()?.subscribe();
    Some((chunk_rx, done_rx))
}

/// Sender the HTTP server uses to inject messages from the browser.
pub fn input_sender() -> Option<mpsc::UnboundedSender<String>> {
    INPUT_TX.get().cloned()
}

/// Non-blocking poll — called by TUI/CLI loop on each iteration.
pub fn try_recv() -> Option<String> {
    INPUT_RX.get()?.lock().ok()?.try_recv().ok()
}
