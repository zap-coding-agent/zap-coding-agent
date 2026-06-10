//! Mock `LlmProvider` for deterministic agent-loop tests.
//!
//! Tests script a sequence of `ApiResponse`s via [`MockClient::with_script`] or
//! [`MockClient::push`], then drive `Session::handle_user_turn` against the mock.
//! Each `send()` call pops the next scripted response and records the inputs
//! (system, messages, tools) so assertions can verify the prompt assembly.
//!
//! `MockClient` is cheap to clone — it holds an `Arc` of the shared queue and
//! call log — so a test can keep one handle for assertions and pass another
//! into the [`crate::session::Session`].

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use super::{ApiResponse, BeforeOutput, ContentBlock, LlmProvider, Message, Usage};

/// One recorded call to [`MockClient::send`].
#[derive(Debug, Clone)]
pub struct RecordedCall {
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<Value>,
    pub thinking_budget: u32,
}

struct MockState {
    responses: Mutex<VecDeque<ApiResponse>>,
    calls: Mutex<Vec<RecordedCall>>,
}

#[derive(Clone)]
pub struct MockClient {
    state: Arc<MockState>,
}

impl MockClient {
    pub fn with_script(responses: Vec<ApiResponse>) -> Self {
        Self {
            state: Arc::new(MockState {
                responses: Mutex::new(responses.into_iter().collect()),
                calls: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn push(&self, response: ApiResponse) {
        self.state.responses.lock().unwrap().push_back(response);
    }

    pub fn call_count(&self) -> usize {
        self.state.calls.lock().unwrap().len()
    }

    pub fn recorded_calls(&self) -> Vec<RecordedCall> {
        self.state.calls.lock().unwrap().clone()
    }

    /// Build a text-only assistant response that ends the turn (`stop_reason="end_turn"`).
    pub fn text(text: impl Into<String>) -> ApiResponse {
        ApiResponse {
            content: vec![ContentBlock::Text { text: text.into() }],
            stop_reason: "end_turn".to_string(),
            usage: Some(Usage::default()),
        }
    }

    /// Build a tool-call assistant response (`stop_reason="tool_use"`).
    pub fn tool_call(id: impl Into<String>, name: impl Into<String>, input: Value) -> ApiResponse {
        ApiResponse {
            content: vec![ContentBlock::ToolUse {
                id: id.into(),
                name: name.into(),
                input,
            }],
            stop_reason: "tool_use".to_string(),
            usage: Some(Usage::default()),
        }
    }
}

#[async_trait]
impl LlmProvider for MockClient {
    async fn send(
        &self,
        system: &str,
        messages: &[Message],
        tools: &[Value],
        before_output: Option<BeforeOutput>,
        thinking_budget: u32,
    ) -> Result<ApiResponse> {
        if let Some(cb) = before_output {
            cb();
        }
        self.state.calls.lock().unwrap().push(RecordedCall {
            system: system.to_string(),
            messages: messages.to_vec(),
            tools: tools.to_vec(),
            thinking_budget,
        });
        let next = self.state.responses.lock().unwrap().pop_front();
        Ok(next.unwrap_or_else(|| Self::text("(mock: queue empty, default end-turn)")))
    }
}
