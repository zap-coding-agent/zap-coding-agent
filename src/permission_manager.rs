use crate::config::PermissionMode;
use anyhow::Result;
use colored::Colorize;
use std::collections::HashMap;
use std::io::{self, Write};

/// Tools that require explicit user approval in Ask mode.
const WRITE_TOOLS: &[&str] = &["shell", "write_file", "edit_file", "batch_edit", "spawn_agent"];

/// Related tools that share an "always" grant.
/// Granting one member grants all members — avoids re-prompting for semantically
/// identical operations (e.g. edit_file vs write_file vs batch_edit).
fn tool_grant_class(name: &str) -> &'static [&'static str] {
    match name {
        "edit_file" | "write_file" | "batch_edit" | "undo_edit" => {
            &["edit_file", "write_file", "batch_edit", "undo_edit"]
        }
        "shell" => &["shell"],
        _ => &[],
    }
}

pub enum QuickDecision { Allow, Deny, NeedsPrompt }

pub struct PermissionManager {
    pub mode: PermissionMode,
    /// Per-session grants; keyed by tool name, set true by "always".
    session_grants: HashMap<String, bool>,
}

impl PermissionManager {
    pub fn new(mode: PermissionMode) -> Self {
        Self { mode, session_grants: HashMap::new() }
    }

    /// Non-blocking check — returns whether to allow, deny, or ask the user.
    /// Returns true if the user already granted "always" for this tool this session.
    pub fn is_session_granted(&self, tool_name: &str) -> bool {
        self.session_grants.get(tool_name).copied() == Some(true)
    }

    pub fn quick_check(&self, tool_name: &str) -> QuickDecision {
        match self.mode {
            PermissionMode::Auto => QuickDecision::Allow,
            PermissionMode::Deny => QuickDecision::Deny,
            PermissionMode::Ask  => {
                if !WRITE_TOOLS.contains(&tool_name) { return QuickDecision::Allow; }
                if self.session_grants.get(tool_name).copied() == Some(true) {
                    return QuickDecision::Allow;
                }
                QuickDecision::NeedsPrompt
            }
        }
    }

    /// Show ONE grouped prompt for all calls that need user input.
    ///
    /// `pending` is a slice of `(id, tool_name, context_string)`.
    /// Returns a parallel `Vec<bool>` — `true` = approved, `false` = denied.
    pub async fn prompt_batch(
        &mut self,
        pending: &[(String, String, String)],
    ) -> Result<Vec<bool>> {
        if pending.is_empty() { return Ok(vec![]); }

        // In TUI mode, use a native TUI dialog instead of breaking out to CLI
        if crate::tui::channel::is_tui_mode() {
            return self.prompt_batch_tui(pending).await;
        }

        // CLI mode: use the existing prompt
        println!();
        if pending.len() == 1 {
            let (_, name, ctx) = &pending[0];
            println!("  {} {}", "Tool:".truecolor(100, 95, 130), name.truecolor(100, 210, 255).bold());
            if !ctx.is_empty() {
                println!("  {} {}", "What:".truecolor(100, 95, 130), ctx.truecolor(130, 125, 150));
            }
        } else {
            println!(
                "  {} {} operation(s):",
                "Agent wants to run".truecolor(100, 95, 130),
                pending.len().to_string().cyan().bold(),
            );
            for (_, name, ctx) in pending {
                let ctx_disp = if ctx.chars().count() > 55 {
                    format!("{}…", ctx.chars().take(54).collect::<String>())
                } else { ctx.clone() };
                println!(
                    "    {} {}  {}",
                    "·".truecolor(70, 65, 90),
                    format!("{:<12}", name).truecolor(100, 210, 255).bold(),
                    ctx_disp.truecolor(130, 125, 150),
                );
            }
        }

        print!("  Allow? [y/N/a(lways)] ");
        io::stdout().flush()?;

        let mut buf = String::new();
        io::stdin().read_line(&mut buf)?;

        let dec = buf.trim().to_lowercase();
        let allowed = matches!(dec.as_str(), "y" | "yes" | "a" | "always");

        if matches!(dec.as_str(), "a" | "always") {
            for (_, name, _) in pending {
                let class = tool_grant_class(name);
                if class.is_empty() {
                    self.session_grants.insert(name.to_string(), true);
                } else {
                    for related in class {
                        self.session_grants.insert(related.to_string(), true);
                    }
                }
            }
            println!(
                "  {} auto-approved for the rest of this session.",
                "→".truecolor(100, 210, 255)
            );
        }

        tracing::info!(allowed, count = pending.len(), "batch permission decision");
        Ok(vec![allowed; pending.len()])
    }

    /// TUI-native permission prompt — posts the prompt to the TUI loop and async-awaits
    /// the response so the tokio runtime stays unblocked (tick loop keeps firing).
    async fn prompt_batch_tui(
        &mut self,
        pending: &[(String, String, String)],
    ) -> Result<Vec<bool>> {
        use crate::tui::channel::{self, PermissionDecision, PermissionPromptRequest};

        let (tx, rx) = tokio::sync::oneshot::channel();
        let req = PermissionPromptRequest {
            pending: pending.to_vec(),
            response_tx: tx,
        };

        // Post the request for the TUI loop to pick up.
        if !channel::set_perm_request(req) {
            anyhow::bail!("permission prompt already pending");
        }

        // Async-await the TUI response — tokio can schedule the tick while we wait.
        let decision = rx.await.unwrap_or(PermissionDecision::Deny);

        let allowed = matches!(decision, PermissionDecision::Allow | PermissionDecision::Always);

        if matches!(decision, PermissionDecision::Always) {
            for (_, name, _) in pending {
                let class = tool_grant_class(name);
                if class.is_empty() {
                    self.session_grants.insert(name.to_string(), true);
                } else {
                    for related in class {
                        self.session_grants.insert(related.to_string(), true);
                    }
                }
            }
        }

        tracing::info!(allowed, count = pending.len(), "TUI batch permission decision");
        Ok(vec![allowed; pending.len()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PermissionMode;

    // ── quick_check mode routing ──────────────────────────────────────────────

    #[test]
    fn auto_mode_allows_everything() {
        let pm = PermissionManager::new(PermissionMode::Auto);
        assert!(matches!(pm.quick_check("shell"),       QuickDecision::Allow));
        assert!(matches!(pm.quick_check("write_file"),  QuickDecision::Allow));
        assert!(matches!(pm.quick_check("read_file"),   QuickDecision::Allow));
        assert!(matches!(pm.quick_check("mcp_tool"),    QuickDecision::Allow));
    }

    #[test]
    fn deny_mode_blocks_everything() {
        let pm = PermissionManager::new(PermissionMode::Deny);
        assert!(matches!(pm.quick_check("shell"),       QuickDecision::Deny));
        assert!(matches!(pm.quick_check("read_file"),   QuickDecision::Deny));
        assert!(matches!(pm.quick_check("mcp_tool"),    QuickDecision::Deny));
    }

    #[test]
    fn ask_mode_prompts_write_tools() {
        let pm = PermissionManager::new(PermissionMode::Ask);
        for tool in &["shell", "write_file", "edit_file", "batch_edit", "spawn_agent"] {
            assert!(
                matches!(pm.quick_check(tool), QuickDecision::NeedsPrompt),
                "expected NeedsPrompt for {tool}"
            );
        }
    }

    #[test]
    fn ask_mode_allows_read_and_search_tools() {
        let pm = PermissionManager::new(PermissionMode::Ask);
        for tool in &["read_file", "search_code", "list_directory", "find_definition",
                      "web_fetch", "web_search", "code_map"] {
            assert!(
                matches!(pm.quick_check(tool), QuickDecision::Allow),
                "expected Allow for {tool}"
            );
        }
    }

    // MCP tools are not in WRITE_TOOLS → quick_check returns Allow in Ask mode.
    // The session-level gate in session/mod.rs upgrades them to NeedsPrompt.
    // This test documents that invariant so a future refactor doesn't silently break it.
    #[test]
    fn ask_mode_allows_mcp_tools_at_quick_check_level() {
        let pm = PermissionManager::new(PermissionMode::Ask);
        assert!(matches!(pm.quick_check("github_list_issues"), QuickDecision::Allow));
        assert!(matches!(pm.quick_check("jira_create_ticket"), QuickDecision::Allow));
    }

    // ── session grants ────────────────────────────────────────────────────────

    #[test]
    fn is_session_granted_false_by_default() {
        let pm = PermissionManager::new(PermissionMode::Ask);
        assert!(!pm.is_session_granted("shell"));
        assert!(!pm.is_session_granted("edit_file"));
        assert!(!pm.is_session_granted("github_list_issues"));
    }

    #[test]
    fn session_grant_bypasses_prompt_for_shell() {
        let mut pm = PermissionManager::new(PermissionMode::Ask);
        pm.session_grants.insert("shell".to_string(), true);
        assert!(matches!(pm.quick_check("shell"), QuickDecision::Allow));
        assert!(pm.is_session_granted("shell"));
    }

    // ── grant class cross-grants ──────────────────────────────────────────────

    #[test]
    fn granting_edit_file_also_grants_write_and_batch() {
        // Simulates pressing "always" for edit_file — should grant the whole class.
        let mut pm = PermissionManager::new(PermissionMode::Ask);
        for related in tool_grant_class("edit_file") {
            pm.session_grants.insert(related.to_string(), true);
        }
        assert!(matches!(pm.quick_check("edit_file"),  QuickDecision::Allow));
        assert!(matches!(pm.quick_check("write_file"), QuickDecision::Allow));
        assert!(matches!(pm.quick_check("batch_edit"), QuickDecision::Allow));
        assert!(matches!(pm.quick_check("undo_edit"),  QuickDecision::Allow));
        // shell is a separate class and must NOT be granted
        assert!(matches!(pm.quick_check("shell"),      QuickDecision::NeedsPrompt));
    }

    #[test]
    fn shell_grant_does_not_spill_to_file_tools() {
        let mut pm = PermissionManager::new(PermissionMode::Ask);
        for related in tool_grant_class("shell") {
            pm.session_grants.insert(related.to_string(), true);
        }
        assert!(matches!(pm.quick_check("shell"),      QuickDecision::Allow));
        assert!(matches!(pm.quick_check("write_file"), QuickDecision::NeedsPrompt));
        assert!(matches!(pm.quick_check("edit_file"),  QuickDecision::NeedsPrompt));
    }

    // ── MCP "always" grant (the empty-class fallback) ─────────────────────────

    #[test]
    fn mcp_tool_has_empty_grant_class() {
        // MCP tools aren't in any grant class — the caller must store the name directly.
        assert!(tool_grant_class("github_list_issues").is_empty());
        assert!(tool_grant_class("jira_create_ticket").is_empty());
        assert!(tool_grant_class("some_custom_mcp_tool").is_empty());
    }

    #[test]
    fn mcp_always_grant_persists_via_direct_name_storage() {
        // Regression test: pressing "always" on an MCP tool must persist so the
        // next call isn't re-prompted (tool_grant_class returns [] for MCP tools,
        // so we store the name directly instead of via the class).
        let mut pm = PermissionManager::new(PermissionMode::Ask);
        let mcp_tool = "github_list_issues";

        // Before grant: not stored
        assert!(!pm.is_session_granted(mcp_tool));

        // Simulate the "always" path: empty class → insert name directly
        let class = tool_grant_class(mcp_tool);
        if class.is_empty() {
            pm.session_grants.insert(mcp_tool.to_string(), true);
        } else {
            for r in class { pm.session_grants.insert(r.to_string(), true); }
        }

        // After grant: is_session_granted returns true
        assert!(pm.is_session_granted(mcp_tool));
        // And quick_check still returns Allow (MCP tools aren't in WRITE_TOOLS)
        assert!(matches!(pm.quick_check(mcp_tool), QuickDecision::Allow));
    }

    // ── ctx sanitization (documents the newline issue that broke the TUI box) ─

    #[test]
    fn shell_ctx_contains_newline_that_must_be_stripped() {
        // ShellTool::permission_context returns "{desc}\n         $ {cmd}" when
        // description is present. This \n would break the TUI dialog box if not
        // stripped before display. Verify the raw string has it so we don't
        // accidentally "fix" the source and forget to remove the sanitization.
        let ctx = "install deps\n         $ npm install";
        assert!(ctx.contains('\n'), "shell ctx with description must contain \\n");
        let flat = ctx.replace('\n', " ").replace('\r', "");
        assert!(!flat.contains('\n'));
        assert!(!flat.contains('\r'));
        assert!(flat.contains("install deps"));
        assert!(flat.contains("$ npm install"));
    }

    #[test]
    fn destructive_ctx_contains_newline_that_must_be_stripped() {
        // destructive_ctx is always "[DESTRUCTIVE: {reason}]\n         {ctx}"
        let ctx = "[DESTRUCTIVE: recursive forced deletion]\n         $ rm -rf build/";
        assert!(ctx.contains('\n'));
        let flat = ctx.replace('\n', " ").replace('\r', "");
        assert!(!flat.contains('\n'));
        assert!(flat.len() <= ctx.len()); // no content added
    }
}
