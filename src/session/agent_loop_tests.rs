//! Deterministic agent-loop tests against [`crate::llm_client::mock::MockClient`].
//!
//! These cover the contract of [`Session::handle_user_turn`] — that it makes the
//! right number of LLM calls, executes tools when the model asks, terminates on
//! `end_turn`, and is bounded by `MAX_TURNS` for runaway tool loops.

use std::io::Write;

use serde_json::json;

use crate::config::{Config, PermissionMode};
use crate::llm_client::mock::MockClient;
use crate::llm_client::{ContentBlock, LlmProvider};

use super::{Session, MAX_TURNS};

fn test_config() -> Config {
    Config {
        model: "test-model".to_string(),
        permission_mode: PermissionMode::Auto,
        is_subagent: true,
        budget: None,
        ..Default::default()
    }
}

#[tokio::test]
async fn single_text_turn_makes_one_call_and_appends_assistant_message() {
    let mock = MockClient::with_script(vec![MockClient::text("hello back")]);
    let session_client: Box<dyn LlmProvider> = Box::new(mock.clone());
    let mut session = Session::new_for_test(&test_config(), session_client).expect("session ctor");

    session.handle_user_turn("hi").await.expect("turn ran");

    assert_eq!(mock.call_count(), 1, "exactly one LLM call for a text-only turn");
    // user + assistant
    assert_eq!(session.messages.len(), 2);
    assert_eq!(session.messages[0].role, "user");
    assert_eq!(session.messages[1].role, "assistant");
    let assistant_text = session.messages[1].content.iter().find_map(|b| {
        if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None }
    });
    assert_eq!(assistant_text, Some("hello back"));
}

#[tokio::test]
async fn one_tool_round_executes_tool_and_loops_back() {
    // Stage a temp file the model "asks" to read.
    let mut tmp = tempfile::NamedTempFile::new().expect("tempfile");
    writeln!(tmp, "line one").unwrap();
    writeln!(tmp, "line two").unwrap();
    let path = tmp.path().to_string_lossy().to_string();

    let mock = MockClient::with_script(vec![
        MockClient::tool_call("call_1", "read_file", json!({ "path": path })),
        MockClient::text("done reading"),
    ]);
    let session_client: Box<dyn LlmProvider> = Box::new(mock.clone());
    let mut session = Session::new_for_test(&test_config(), session_client).expect("session ctor");

    session.handle_user_turn("read it").await.expect("turn ran");

    assert_eq!(mock.call_count(), 2, "tool round = one LLM call + one follow-up");

    // Messages: user, assistant (tool_use), user (tool_result), assistant (final text).
    assert_eq!(session.messages.len(), 4, "user + tool_use + tool_result + assistant");

    let tool_result = session.messages[2].content.iter().find_map(|b| {
        if let ContentBlock::ToolResult { content, tool_use_id } = b {
            Some((tool_use_id.as_str(), content.as_str()))
        } else { None }
    });
    let (tool_use_id, body) = tool_result.expect("tool_result block present");
    assert_eq!(tool_use_id, "call_1");
    assert!(body.contains("line one"), "tool actually read the file: {body}");
    assert!(body.contains("line two"));

    // The second LLM call must have included the tool_result message.
    let calls = mock.recorded_calls();
    let second_call_msgs = &calls[1].messages;
    let has_tool_result = second_call_msgs.iter().any(|m| {
        m.content.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. }))
    });
    assert!(has_tool_result, "second call must carry the tool result back to the model");
}

#[tokio::test]
async fn runaway_tool_calls_stop_at_max_turns() {
    // Seed enough tool calls that the loop would run forever without the cap.
    let mut script: Vec<crate::llm_client::ApiResponse> = (0..MAX_TURNS + 5)
        .map(|i| MockClient::tool_call(format!("call_{i}"), "read_file", json!({ "path": "/dev/null" })))
        .collect();
    // A trailing text — should never be reached.
    script.push(MockClient::text("should not see this"));

    let mock = MockClient::with_script(script);
    let session_client: Box<dyn LlmProvider> = Box::new(mock.clone());
    let mut session = Session::new_for_test(&test_config(), session_client).expect("session ctor");

    session.handle_user_turn("loop please").await.expect("turn ran");

    // The loop body runs MAX_TURNS times — one LLM call per iteration.
    assert_eq!(
        mock.call_count(),
        MAX_TURNS,
        "loop must stop exactly at MAX_TURNS",
    );
}
