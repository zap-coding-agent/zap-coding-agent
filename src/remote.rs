/// Remote control HTTP server — serves a web chat UI and tunnels it publicly.
///
/// `/remote [port]` starts the server, launches a tunnel (ngrok → localhost.run),
/// and prints a URL you can open on any device to drive the current zap session.
use anyhow::{Context, Result};
use axum::{
    Router,
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, State},
    response::{Html, IntoResponse},
    routing::get,
};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;

// ── HTML UI ────────────────────────────────────────────────────────────────────

const UI_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>zap remote</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{background:#1a1824;color:#d4d0e0;font-family:-apple-system,BlinkMacSystemFont,'SF Pro',system-ui,sans-serif;height:100dvh;display:flex;flex-direction:column}
#header{padding:12px 16px;border-bottom:1px solid #2a2640;display:flex;align-items:center;gap:10px}
.dot{width:9px;height:9px;border-radius:50%;background:#3d3850;transition:background .3s}
.dot.on{background:#50d280}
#title{color:#c88c30;font-weight:700;font-size:16px;letter-spacing:-.3px}
#status-text{color:#6a6480;font-size:13px;margin-left:auto}
#messages{flex:1;overflow-y:auto;padding:16px;display:flex;flex-direction:column;gap:10px;scroll-behavior:smooth}
.bubble{max-width:88%;padding:10px 14px;border-radius:14px;font-size:15px;line-height:1.55;white-space:pre-wrap;word-break:break-word}
.bubble.user{align-self:flex-end;background:#2d2840;color:#d8d4e8;border-bottom-right-radius:4px}
.bubble.bot{align-self:flex-start;background:#1e1b2c;color:#c8c4dc;border:1px solid #2d2840;border-bottom-left-radius:4px;min-width:40px}
.bubble.bot.streaming::after{content:'▋';color:#c88c30;animation:blink 1s step-end infinite}
@keyframes blink{50%{opacity:0}}
#foot{padding:12px 14px;border-top:1px solid #2a2640;display:flex;gap:8px;align-items:flex-end}
#inp{flex:1;background:#221e30;border:1px solid #3a3550;border-radius:12px;color:#d4d0e0;font-size:15px;padding:10px 14px;resize:none;outline:none;max-height:130px;line-height:1.45;font-family:inherit}
#inp:focus{border-color:#5a5070}
#inp:disabled{opacity:.45}
#btn{background:#c88c30;border:none;border-radius:12px;color:#1a1824;font-size:20px;width:44px;height:44px;cursor:pointer;flex-shrink:0;transition:opacity .15s;display:flex;align-items:center;justify-content:center}
#btn:disabled{opacity:.3;cursor:not-allowed}
code{background:#2a2640;padding:1px 6px;border-radius:4px;font-size:13px;font-family:'SF Mono',Menlo,monospace}
</style>
</head>
<body>
<div id="header">
  <div class="dot" id="dot"></div>
  <span id="title">⚡ zap remote</span>
  <span id="status-text">connecting…</span>
</div>
<div id="messages"></div>
<div id="foot">
  <textarea id="inp" rows="1" placeholder="Message zap…" disabled></textarea>
  <button id="btn" disabled>↑</button>
</div>
<script>
const msgs=document.getElementById('messages'),
      inp=document.getElementById('inp'),
      btn=document.getElementById('btn'),
      dot=document.getElementById('dot'),
      st=document.getElementById('status-text');
let ws,cur=null,busy=false;

function setReady(ok){
  dot.className='dot'+(ok?' on':'');
  st.textContent=ok?'connected':'reconnecting…';
  inp.disabled=!ok||busy;
  btn.disabled=!ok||busy;
  if(ok)inp.focus();
}

function addBubble(role,text=''){
  const d=document.createElement('div');
  d.className='bubble '+role;
  d.textContent=text;
  msgs.appendChild(d);
  msgs.scrollTop=msgs.scrollHeight;
  return d;
}

function connect(){
  const proto=location.protocol==='https:'?'wss://':'ws://';
  ws=new WebSocket(proto+location.host+'/ws');
  ws.onopen=()=>setReady(true);
  ws.onclose=()=>{setReady(false);setTimeout(connect,2000)};
  ws.onerror=()=>ws.close();
  ws.onmessage=e=>{
    const d=JSON.parse(e.data);
    if(d.t==='c'){
      if(!cur){cur=addBubble('bot');cur.classList.add('streaming')}
      cur.textContent+=d.v;
      msgs.scrollTop=msgs.scrollHeight;
    }else if(d.t==='d'){
      if(cur)cur.classList.remove('streaming');
      cur=null;busy=false;
      inp.disabled=false;btn.disabled=false;inp.focus();
    }
  };
}

function send(){
  const t=inp.value.trim();
  if(!t||busy||ws.readyState!==1)return;
  addBubble('user',t);
  ws.send(JSON.stringify({t:'m',v:t}));
  inp.value='';inp.style.height='';
  busy=true;inp.disabled=true;btn.disabled=true;
}

btn.onclick=send;
inp.onkeydown=e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();send()}};
inp.oninput=()=>{inp.style.height='';inp.style.height=Math.min(inp.scrollHeight,130)+'px'};
connect();
</script>
</body>
</html>"#;

// ── Axum state ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    input_tx: tokio::sync::mpsc::UnboundedSender<String>,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn serve_ui() -> impl IntoResponse {
    Html(UI_HTML)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sink, mut stream) = {
        use futures_util::StreamExt;
        socket.split()
    };

    // Subscribe to LLM chunks and done signals for this connection.
    let (mut chunk_rx, mut done_rx) = match crate::remote_channel::subscribe() {
        Some(pair) => pair,
        None       => return,
    };

    let input_tx = state.input_tx.clone();

    // Spawn: forward LLM output → WebSocket
    let out_task = tokio::spawn(async move {
        use futures_util::SinkExt;
        loop {
            tokio::select! {
                Ok(chunk) = chunk_rx.recv() => {
                    let msg: String = serde_json::json!({"t":"c","v":chunk}).to_string();
                    if sink.send(Message::Text(msg)).await.is_err() { break; }
                }
                Ok(()) = done_rx.recv() => {
                    let msg: String = serde_json::json!({"t":"d"}).to_string();
                    if sink.send(Message::Text(msg)).await.is_err() { break; }
                }
                else => break,
            }
        }
    });

    // Main: receive messages from browser → session input channel
    use futures_util::StreamExt;
    while let Some(Ok(msg)) = stream.next().await {
        if let Message::Text(text) = msg {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                if val["t"].as_str() == Some("m") {
                    if let Some(v) = val["v"].as_str() {
                        let _ = input_tx.send(v.to_string());
                    }
                }
            }
        }
    }

    out_task.abort();
}

// ── Server startup ────────────────────────────────────────────────────────────

/// Bind to `port` (0 = random), return the actual port used.
pub async fn start_server(port: u16) -> Result<u16> {
    let input_tx = crate::remote_channel::input_sender()
        .context("remote_channel not activated — call remote_channel::activate() first")?;

    let state = Arc::new(AppState { input_tx });

    let app = Router::new()
        .route("/",   get(serve_ui))
        .route("/ws", get(ws_handler))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await
        .with_context(|| format!("could not bind to port {}", port))?;
    let actual_port = listener.local_addr()?.port();

    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    Ok(actual_port)
}

// ── Tunnel ────────────────────────────────────────────────────────────────────

/// Try ngrok first (queries its local API on :4040), then fall back to
/// localhost.run via SSH. Returns the public HTTPS URL.
pub async fn launch_tunnel(port: u16) -> Result<String> {
    // ── ngrok ─────────────────────────────────────────────────────────────────
    if let Ok(ngrok_path) = which_ngrok() {
        // Start ngrok in background.
        let _ = tokio::process::Command::new(&ngrok_path)
            .args(["http", &port.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        // Poll ngrok's local API until the tunnel is up (max 5s).
        for _ in 0..10 {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if let Ok(url) = ngrok_url().await {
                return Ok(url);
            }
        }
    }

    // ── localhost.run (SSH, always available) ─────────────────────────────────
    localhost_run_tunnel(port).await
}

fn which_ngrok() -> Result<String> {
    // Check common locations.
    for path in [
        "/opt/homebrew/bin/ngrok",
        "/usr/local/bin/ngrok",
        "/usr/bin/ngrok",
    ] {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }
    // Also try PATH via `which`.
    let out = std::process::Command::new("which").arg("ngrok").output()?;
    if out.status.success() {
        let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !p.is_empty() { return Ok(p); }
    }
    anyhow::bail!("ngrok not found")
}

async fn ngrok_url() -> Result<String> {
    let body = crate::http::client()
        .get("http://127.0.0.1:4040/api/tunnels")
        .send()
        .await?
        .text()
        .await?;
    let val: serde_json::Value = serde_json::from_str(&body)?;
    val["tunnels"]
        .as_array()
        .and_then(|arr| arr.iter().find_map(|t| {
            if t["proto"].as_str() == Some("https") {
                t["public_url"].as_str().map(|s| s.to_string())
            } else {
                None
            }
        }))
        .context("no https tunnel in ngrok API response")
}

async fn localhost_run_tunnel(port: u16) -> Result<String> {
    use tokio::io::AsyncBufReadExt;

    let mut child = tokio::process::Command::new("ssh")
        .args([
            "-o", "StrictHostKeyChecking=no",
            "-o", "ServerAliveInterval=30",
            "-R", &format!("80:localhost:{}", port),
            "nokey@localhost.run",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn ssh for localhost.run tunnel")?;

    // Read lines from stderr (localhost.run prints the URL there) and stdout.
    let stderr = child.stderr.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    // Keep child alive.
    tokio::spawn(async move { let _ = child.wait().await; });

    // Parse the URL from either stream (within 10s).
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let tx2 = tx.clone();

    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(l)) = lines.next_line().await { let _ = tx.send(l); }
    });
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        while let Ok(Some(l)) = lines.next_line().await { let _ = tx2.send(l); }
    });

    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv()).await {
            Ok(Some(line)) => {
                // localhost.run prints something like:
                //   tunneled with tls termination, https://abc123.lhr.life
                if let Some(url) = extract_https_url(&line) {
                    return Ok(url);
                }
            }
            _ => {}
        }
    }

    anyhow::bail!("timed out waiting for localhost.run tunnel URL")
}

fn extract_https_url(line: &str) -> Option<String> {
    // Match "https://..." anywhere in the line.
    let start = line.find("https://")?;
    let rest  = &line[start..];
    // URL ends at whitespace or end of string.
    let end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    Some(rest[..end].to_string())
}
