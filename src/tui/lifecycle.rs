use std::io::Stdout;

use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

pub(super) fn suspend_tui(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

pub(super) fn resume_tui(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::EnterAlternateScreen
    )?;
    terminal.clear()?;
    Ok(())
}

/// Open a native folder picker dialog and return the chosen path.
pub(super) fn open_dir_picker() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let current_dir = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "/".to_string());

        let script = format!(
            r#"POSIX path of (choose folder with prompt "Select a directory:" default location POSIX file "{}")"#,
            current_dir
        );

        let output = std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .ok()?;

        if output.status.success() {
            let path = String::from_utf8(output.stdout).ok()?;
            let path = path.trim().trim_end_matches('/');
            if !path.is_empty() && std::path::Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
        None
    }

    #[cfg(target_os = "windows")]
    {
        let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = 'Select a directory'
$dialog.ShowNewFolderButton = $true
if ($dialog.ShowDialog() -eq 'OK') {
    Write-Output $dialog.SelectedPath
}
"#;
        let output = std::process::Command::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(script)
            .output()
            .ok()?;

        if output.status.success() {
            let path = String::from_utf8(output.stdout).ok()?;
            let path = path.trim();
            if !path.is_empty() && std::path::Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
        None
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Run task planning in the normal terminal (TUI suspended) and return the
/// intro message to prime the first TUI turn, or None if aborted.
pub(super) async fn run_task_planning_tui(session: &crate::session::Session) -> Option<String> {
    match crate::task_planner::run_task_planning(
        session.client.as_ref(),
        &session.model,
        &session.skills,
    )
    .await
    {
        Ok(Some(plan)) => {
            println!();
            print!("  \x1b[2m── Press Enter to enter task session ──\x1b[0m ");
            use std::io::Write;
            std::io::stdout().flush().ok();
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).ok();
            Some(format!(
                "I'm starting a task session. Goal: {}\n\n\
                 The tasks.md has been created at .zap/tasks/{}/tasks.md\n\
                 Please read it and confirm you understand the plan before we start.",
                plan.goal, plan.folder_name
            ))
        }
        Ok(None) => None,
        Err(e) => {
            use colored::Colorize;
            println!("  {} Planning failed: {} — continuing in Vibe mode.", "⚠".yellow(), e);
            use std::io::Write;
            std::io::stdout().flush().ok();
            None
        }
    }
}

/// Paste clipboard image into the current message as a staged attachment.
pub(super) fn handle_paste_image(
    app: &mut super::app::App,
    session: &mut crate::session::Session,
) {
    use super::app::{MsgRole, UiBlock, UiMessage};
    let tmp = "/tmp/zap_clipboard_paste.png";
    let ok = crate::session::commands::paste_clipboard_image(tmp);
    if ok && std::path::Path::new(tmp).exists() {
        match std::fs::read(tmp) {
            Ok(bytes) => {
                // Guard: reject tiny/corrupt files (e.g. pngpaste writing text as a malformed PNG)
                const MIN_IMAGE_BYTES: usize = 128;
                if bytes.len() < MIN_IMAGE_BYTES {
                    let _ = std::fs::remove_file(tmp);
                } else {
                    use base64::Engine;
                    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    let kb = bytes.len() / 1024;
                    session.staged_images.push(("image/png".to_string(), data));
                    app.messages.push(UiMessage {
                        role: MsgRole::Assistant,
                        blocks: vec![UiBlock::Text(format!(
                            "✓ Image attached ({} KB) — it will be sent with your next message.", kb
                        ))],
                    });
                    app.auto_scroll = true;
                    return;
                }
            }
            Err(e) => {
                app.messages.push(UiMessage {
                    role: MsgRole::Assistant,
                    blocks: vec![UiBlock::Text(format!("✗ Failed to read clipboard image: {}", e))],
                });
                app.auto_scroll = true;
                return;
            }
        }
    }

    // Image paste failed (or produced a tiny corrupt file) — try text paste.
    if let Some(text) = crate::session::commands::paste_clipboard_text() {
        // Strip all trailing newlines/carriage-returns (clipboard tools often add one).
        let mut trimmed = text.as_str();
        while trimmed.ends_with('\n') || trimmed.ends_with('\r') {
            trimmed = &trimmed[..trimmed.len() - 1];
        }
        if !trimmed.is_empty() {
            let byte_idx = super::input::char_to_byte_idx(&app.input, app.cursor);
            app.input.insert_str(byte_idx, trimmed);
            app.cursor = app.input.chars().count();
            app.picker_sel = 0;
            return;
        } else {
            app.messages.push(UiMessage {
                role: MsgRole::Assistant,
                blocks: vec![UiBlock::Text(
                    "✗ Clipboard is empty. Copy some text or a screenshot first, then press Ctrl+V again."
                        .to_string(),
                )],
            });
            app.auto_scroll = true;
            return;
        }
    }

    // Neither image nor text available.
    app.messages.push(UiMessage {
        role: MsgRole::Assistant,
        blocks: vec![UiBlock::Text(
            "✗ No image or text in clipboard. Copy something first, then press Ctrl+V again."
                .to_string(),
        )],
    });
    app.auto_scroll = true;
}

/// Show instructions for using a Gemini API key instead of gcloud auth.
pub(super) fn handle_gemini_auth_apikey(app: &mut super::app::App) {
    use super::app::{MsgRole, UiBlock, UiMessage};
    app.gemini_auth_prompt = false;
    app.gemini_reauth = false;
    app.messages.push(UiMessage {
        role: MsgRole::Assistant,
        blocks: vec![UiBlock::Text(
            "To use Gemini with an API key:\n\
             \n\
             1. Get a free key at:  aistudio.google.com/apikey\n\
             2. Run in your terminal:\n\
             \n\
               export GOOGLE_API_KEY=your-key-here\n\
             \n\
             Then open /provider — Gemini will show ✓ ready."
                .to_string(),
        )],
    });
    app.auto_scroll = true;
}

/// Handle SelectProvider — builds PendingProviderSwitch and sets app.api_key_input.
pub(super) fn handle_select_provider(
    app: &mut super::app::App,
    config: &crate::config::Config,
    idx: usize,
) {
    use super::app::{MsgRole, PendingProviderSwitch, ProviderKind, UiBlock, UiMessage};
    if let Some(ref picker) = app.provider_picker {
        if let Some(entry) = picker.entries.get(idx) {
            if entry.coming_soon {
                app.messages.push(UiMessage {
                    role: MsgRole::Assistant,
                    blocks: vec![UiBlock::Text(format!(
                        "{} Claude Code (Pro/Max API) — coming 16 Jun 2026.\nUse Anthropic provider with an API key until then.",
                        "◷",
                    ))],
                });
                app.auto_scroll = true;
            } else {
                let slug = entry.slug.clone();
                let name = entry.name.clone();
                let needs_key = entry.needs_key;
                let kind_str = match entry.kind {
                    ProviderKind::Anthropic => "anthropic",
                    ProviderKind::OpenAi    => "openai",
                };
                let provider = match entry.kind {
                    ProviderKind::Anthropic => crate::config::Provider::Anthropic,
                    ProviderKind::OpenAi    => crate::config::Provider::OpenAi,
                };
                let existing_key = config.all_providers.get(&slug)
                    .and_then(|e| e.api_key.clone())
                    .filter(|k| !k.is_empty());
                let models = entry.models.clone();
                let auth_header = entry.auth_header.map(|h| h.to_string());
                let base_url = entry.base_url.clone();
                app.api_key_input = Some(PendingProviderSwitch {
                    slug, name, models, kind_str, provider, base_url, auth_header,
                    input: String::new(),
                    has_existing_key: existing_key.is_some(),
                    picking_model: !needs_key,
                    model_sel: 0,
                    resolved_key: None,
                });
            }
        }
    }
    app.provider_picker = None;
}

/// Run `gcloud auth login` in a subprocess, resume TUI, then auto-switch to Gemini if successful.
pub(super) fn handle_gemini_auth_launch(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    session: &mut crate::session::Session,
    app: &mut super::app::App,
    config: &crate::config::Config,
) -> anyhow::Result<()> {
    use super::app::{MsgRole, UiBlock, UiMessage};
    app.gemini_auth_prompt = false;
    app.gemini_reauth = false;
    suspend_tui(terminal)?;
    let gcloud_candidates: &[&str] = &[
        "gcloud",
        "/opt/homebrew/bin/gcloud",
        "/usr/local/bin/gcloud",
        "/usr/local/google-cloud-sdk/bin/gcloud",
    ];
    let gcloud_bin = gcloud_candidates.iter()
        .find(|&&c| std::process::Command::new(c).arg("--version")
            .output().map(|o| o.status.success()).unwrap_or(false))
        .copied()
        .unwrap_or("gcloud");
    // Use `gcloud auth login` (user credentials, accepted by generativelanguage.googleapis.com).
    let auth_output = std::process::Command::new(gcloud_bin).args(["auth", "login"]).output();
    let auth_status = auth_output.as_ref().map(|o| o.status.success()).unwrap_or(false);
    let auth_stderr = auth_output.as_ref()
        .map(|o| String::from_utf8_lossy(&o.stderr).to_string())
        .unwrap_or_default();
    resume_tui(terminal)?;

    crate::llm_client::auth::invalidate_gcloud_cache();
    let now_ready = crate::llm_client::auth::check_gcloud_adc().is_some();

    if now_ready {
        let model = "gemini-2.0-flash".to_string();
        let base_url = Some("https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".to_string());
        let mut new_config = config.clone();
        new_config.provider = crate::config::Provider::OpenAi;
        new_config.provider_slug = "gemini".to_string();
        new_config.model = model.clone();
        new_config.base_url = base_url.clone();
        new_config.api_key = String::new();
        new_config.all_providers.insert("gemini".to_string(), crate::config::ProviderEntry {
            kind: Some("openai".to_string()),
            model: Some(model.clone()),
            api_key: None,
            base_url: base_url.clone(),
            credential_method: Some("gcloud_adc".to_string()),
            auth_header: Some("x-goog-api-key".to_string()),
        });
        session.client = crate::llm_client::create_client(&new_config);
        session.model = model.clone();
        session.base_url = new_config.base_url.clone();
        session.config = new_config.clone();
        let _ = new_config.save();
        app.model = model.clone();
        app.messages.push(UiMessage {
            role: MsgRole::Assistant,
            blocks: vec![UiBlock::Text(format!(
                "✓ Signed in with gcloud. Switched to Google Gemini · {}", model
            ))],
        });
    } else {
        let msg = if auth_output.is_err() {
            format!(
                "Could not launch gcloud: {}\n\
                 Install the Google Cloud SDK:\n\n  brew install --cask google-cloud-sdk\n\n\
                 Then run in a terminal:\n\n  gcloud auth application-default login",
                auth_output.err().unwrap()
            )
        } else if auth_status {
            "gcloud login completed but credentials were not written.\n\
             Try opening /provider again — Gemini should now show ✓ ready.".to_string()
        } else if auth_stderr.contains("not consented") || auth_stderr.contains("scope") {
            "Google sign-in: consent was not granted for Cloud Platform access.\n\n\
             Please run this in a terminal and make sure to click Allow\n\
             for all permissions on the Google consent page:\n\n\
               gcloud auth application-default login\n\n\
             Then open /provider — Gemini will show ✓ ready.".to_string()
        } else {
            "gcloud login was cancelled or failed.\n\
             Run this in a terminal and try again:\n\n\
               gcloud auth application-default login\n\n\
             Then open /provider — Gemini will show ✓ ready.".to_string()
        };
        app.messages.push(UiMessage {
            role: MsgRole::Assistant,
            blocks: vec![UiBlock::Text(msg)],
        });
    }
    app.auto_scroll = true;
    Ok(())
}

/// Complete a provider switch — update session and app state, save config.
/// Shared by immediate switches (key already saved) and the API key overlay submit path.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_provider_switch(
    session: &mut crate::session::Session,
    app: &mut super::app::App,
    base_config: &crate::config::Config,
    slug: String,
    name: String,
    model: String,
    kind_str: &'static str,
    provider: crate::config::Provider,
    base_url: Option<String>,
    auth_header: Option<String>,
    api_key: Option<String>,
) {
    use super::app::{MsgRole, UiBlock, UiMessage};
    let mut new_config = base_config.clone();
    new_config.provider = provider;
    new_config.provider_slug = slug.clone();
    new_config.model = model.clone();
    new_config.base_url = base_url.clone();
    new_config.api_key = api_key.clone().unwrap_or_default();
    new_config.all_providers.insert(slug.clone(), crate::config::ProviderEntry {
        kind: Some(kind_str.to_string()),
        model: Some(model.clone()),
        api_key: api_key.clone(),
        base_url: base_url.clone(),
        credential_method: None,
        auth_header,
    });
    session.client = crate::llm_client::create_client(&new_config);
    session.model = model.clone();
    session.base_url = new_config.base_url.clone();
    session.config = new_config.clone();
    let _ = new_config.save();
    app.model = model.clone();
    let status = if api_key.is_none() {
        format!("✓ Switched to {} · {}\n⚠ No API key saved — set AGENT_API_KEY env var or open /provider to add one", name, model)
    } else {
        format!("✓ Switched to {} · {}", name, model)
    };
    app.messages.push(UiMessage {
        role: MsgRole::Assistant,
        blocks: vec![UiBlock::Text(status)],
    });
    app.auto_scroll = true;
}
