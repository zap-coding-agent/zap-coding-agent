//! Test-only factory for constructing a minimal [`Session`] without the
//! heavy startup I/O of [`Session::new`].
//!
//! Splits the lengthy real constructor from the small mock constructor so
//! `src/session/mod.rs` stays under the project-wide 600-line cap.

use std::sync::{Arc, Mutex};

use anyhow::Result;

use crate::{
    config::{Config, PermissionMode},
    llm_client::{LlmProvider, Usage},
    permission_manager::PermissionManager,
    persistence,
    tools::ToolRegistry,
};

use super::Session;

impl Session {
    /// Minimal `Session` for deterministic agent-loop tests.
    ///
    /// Skips the heavy parts of [`Session::new`] (skill bootstrap, MCP load,
    /// code-index build, project banners, network init) and lets the caller
    /// inject a [`LlmProvider`] — typically
    /// [`crate::llm_client::mock::MockClient`]. Uses an in-memory sqlite store
    /// and in-memory code index so tests don't touch `~/.zap/agent.db` or the
    /// project's `.zap/code.db`.
    ///
    /// `config.is_subagent` is forced to `true` so the turn loop suppresses
    /// prints and skips startup notices; `permission_mode` is forced to `Auto`
    /// so tool calls don't prompt.
    pub fn new_for_test(config: &Config, client: Box<dyn LlmProvider>) -> Result<Self> {
        let mut cfg = config.clone();
        cfg.is_subagent = true;
        cfg.permission_mode = PermissionMode::Auto;

        let store = persistence::Store::open_in_memory()?;
        let session_id = store.save_session("(test)", &cfg.model)?;

        let tools = ToolRegistry::new(crate::config::SandboxMode::Off);
        let tool_defs = tools.tool_definitions();
        let tool_count = tool_defs.len();

        let code_index = {
            let idx = crate::code_index::CodeIndex::open_in_memory()
                .expect("SQLite in-memory always works");
            Arc::new(Mutex::new(idx))
        };

        let hooks = crate::hooks::HookRunner {
            pre_tool_use: Vec::new(),
            post_tool_use: Vec::new(),
            session_start: Vec::new(),
            session_end: Vec::new(),
            user_prompt_submit: Vec::new(),
        };

        Ok(Self {
            client,
            tools,
            permissions: PermissionManager::new(cfg.permission_mode.clone()),
            system: String::new(),
            tool_defs,
            messages: Vec::new(),
            model: cfg.model.clone(),
            base_url: cfg.base_url.clone(),
            session_usage: Usage::default(),
            turn_count: 0,
            tool_count,
            session_id,
            config: cfg,
            staged_images: Vec::new(),
            skills: Vec::new(),
            domain_scope: std::collections::HashSet::new(),
            pinned_skills: std::collections::HashSet::new(),
            current_branch: "main".to_string(),
            code_index,
            store,
            hooks,
            thinking_budget: 0,
            compact_failures: 0,
            files_changed: Vec::new(),
            startup_notices: Vec::new(),
            skill_trace: Vec::new(),
            dropped_summary: String::new(),
            last_window_start: 0,
            edited_files: std::collections::HashMap::new(),
        })
    }
}
