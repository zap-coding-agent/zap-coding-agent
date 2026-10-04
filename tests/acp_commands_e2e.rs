//! ACP mode end-to-end tests: slash commands.
//!
//! In the TUI these are handled by the front-end, so each route into an editor
//! is exercised here — inline text, captured terminal output, argument-driven
//! versions of picker commands, the goal loop, and terminal-only refusals.

mod acp_support;

use acp_support::{is_update, text_reply, AcpClient, FakeLlm};
use serde_json::json;
use std::time::Duration;

#[test]
fn new_session_announces_slash_commands() {
    let llm = FakeLlm::start(vec![text_reply("ok")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let _sid = c.new_session();
    let m = c.wait_for("available_commands_update", is_update("available_commands_update"));
    let names: Vec<&str> = m["params"]["update"]["availableCommands"].as_array().unwrap().iter()
        .filter_map(|c| c["name"].as_str()).collect();
    for expected in ["init", "index", "provider", "model", "compact", "cost", "goal", "undo", "skill"] {
        assert!(names.contains(&expected), "missing /{expected} in {names:?}");
    }
    // Terminal-only and editor-owned commands are not offered.
    for hidden in ["bg", "agents", "schedule", "unschedule", "remote", "exit", "new"] {
        assert!(!names.contains(&hidden), "/{hidden} should not be offered: {names:?}");
    }
}

#[test]
fn inline_command_returns_text_without_calling_the_model() {
    // The fake LLM would answer "MODEL REPLY" — a command must not reach it.
    let llm = FakeLlm::start(vec![text_reply("MODEL REPLY")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let sid = c.new_session();
    let out = c.say(&sid, "/history");
    assert!(out.contains("messages in history"), "{out:?}");
    assert!(!out.contains("MODEL REPLY"), "{out:?}");
}

#[test]
fn cli_style_command_output_is_captured_into_the_chat() {
    let llm = FakeLlm::start(vec![text_reply("MODEL REPLY")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let sid = c.new_session();
    // /tools reports through println! — it only reaches the client via capture.
    let out = c.say(&sid, "/tools");
    assert!(out.contains("read_file"), "captured output: {out:?}");
    assert!(out.starts_with("```text\n") && out.trim_end().ends_with("```"), "not fenced: {out:?}");
    assert!(!out.contains('\u{1b}'), "ANSI escapes leaked: {out:?}");
    assert!(!out.contains("MODEL REPLY"));

    // Capture must be released afterwards: a normal turn still works and the
    // protocol stream is intact.
    let out = c.say(&sid, "hello");
    assert!(out.contains("MODEL REPLY"), "{out:?}");
}

#[test]
fn terminal_only_commands_explain_themselves() {
    let llm = FakeLlm::start(vec![text_reply("MODEL REPLY")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let sid = c.new_session();
    for cmd in ["/bg write docs", "/agents", "/schedule every 5m: test", "/unschedule 1"] {
        let out = c.say(&sid, cmd);
        assert!(out.contains("isn't available inside an editor"), "{cmd}: {out:?}");
    }
    assert!(c.say(&sid, "/new").contains("new thread"));
}

#[test]
fn permissions_command_updates_the_editor_mode() {
    let llm = FakeLlm::start(vec![text_reply("ok")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[("AGENT_PERMISSION_MODE", "ask")]);
    let sid = c.new_session();
    c.say(&sid, "/permissions auto");
    let m = c.wait_for("current_mode_update", is_update("current_mode_update"));
    assert_eq!(m["params"]["update"]["currentModeId"], "auto");
}

#[test]
fn provider_and_model_commands_list_instead_of_opening_pickers() {
    let llm = FakeLlm::start(vec![text_reply("ok")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let sid = c.new_session();
    let out = c.say(&sid, "/provider");
    assert!(out.contains("**Current:** `openai` · `fake-model`"), "{out:?}");
    let out = c.say(&sid, "/provider nosuch");
    assert!(out.contains("isn't configured yet"), "{out:?}");
    let out = c.say(&sid, "/model");
    assert!(out.contains("**Current model:** `fake-model`"), "{out:?}");
    let out = c.say(&sid, "/model other-model");
    assert!(out.contains("Model switched to: other-model"), "{out:?}");
    assert!(c.say(&sid, "/model").contains("`other-model`"));
}

#[test]
fn goal_runs_turns_until_done_marker() {
    let llm = FakeLlm::start(
        vec![text_reply("working on it"), text_reply("finished ✓ DONE")],
        Duration::ZERO,
    );
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let sid = c.new_session();
    let out = c.say(&sid, "/goal write the docs --max 5");
    assert!(out.contains("Goal — turn 1/5"), "{out:?}");
    assert!(out.contains("Goal — turn 2/5"), "{out:?}");
    assert!(out.contains("✓ Goal complete in 2 turns."), "{out:?}");
    assert!(!out.contains("turn 3/5"), "{out:?}");
}

#[test]
fn goal_stops_at_turn_limit() {
    let llm = FakeLlm::start(vec![text_reply("still going")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let sid = c.new_session();
    let out = c.say(&sid, "/goal never ends --max 2");
    assert!(out.contains("⏹ Goal stopped: 2 turn limit reached."), "{out:?}");
}

#[test]
fn init_indexes_and_writes_project_files() {
    let llm = FakeLlm::start(vec![text_reply("Filled in ZAP.md")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[("AGENT_PERMISSION_MODE", "auto")]);
    std::fs::write(c.project.path().join("main.rs"), "fn main() { helper(); }\nfn helper() {}\n").unwrap();
    let sid = c.new_session();
    let out = c.say(&sid, "/init rust");
    assert!(out.contains("Setting up this project for zap (rust)"), "{out:?}");
    assert!(c.project.path().join("ZAP.md").exists(), "ZAP.md not created; output: {out:?}");
    assert!(c.project.path().join(".zap").is_dir(), ".zap/ not created");
}

#[test]
fn index_and_diff_and_context_commands_report_in_chat() {
    let llm = FakeLlm::start(vec![text_reply("ok")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    std::fs::write(c.project.path().join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
    let sid = c.new_session();
    let out = c.say(&sid, "/index");
    assert!(out.contains("Indexing the codebase"), "{out:?}");
    let out = c.say(&sid, "/diff");
    assert!(out.contains("No diff available"), "{out:?}");
    c.say(&sid, "first question");
    let out = c.say(&sid, "/context");
    assert!(out.contains("**Context:**") && out.contains("first question"), "{out:?}");
}

#[test]
fn list_sessions_returns_prompted_sessions_for_the_project() {
    let llm = FakeLlm::start(vec![text_reply("ok")], Duration::ZERO);
    let mut c = AcpClient::spawn(Some(&llm), &[]);
    let sid = c.new_session();
    c.say(&sid, "remember this session");
    let cwd = c.project.path().to_string_lossy().to_string();
    let r = c.request("session/list", json!({ "cwd": cwd }));
    let sessions = r["result"]["sessions"].as_array().unwrap_or_else(|| panic!("session/list failed: {r}"));
    assert!(sessions.iter().any(|s| s["sessionId"] == sid.as_str()), "{sid} not in {sessions:?}");
}
