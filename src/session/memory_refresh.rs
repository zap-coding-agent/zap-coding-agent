use super::Session;

impl Session {
    /// Replace the `## Agent Memory` section in `self.system` with a fresh read
    /// from the DB. Called after any `memory_set` / `memory_delete` tool call so
    /// the next LLM call in the same session sees updated facts.
    pub(super) fn patch_memory_in_system(&mut self) {
        let new_block = match build_memory_block() {
            Some(b) => b,
            None    => return,
        };

        if let Some(start) = self.system.find("## Agent Memory\n") {
            // Find where this block ends. tail[1..] skips the leading '#' so the
            // search for "\n\n## " cannot match the block's own opening "##".
            // `rel` is the position of the "\n\n## " separator in tail[1..], which
            // means position rel+1 in tail.  We stop there — do NOT consume the
            // "\n\n" itself; it must stay so the next section isn't run on directly.
            let tail = &self.system[start..];
            let block_len = tail[1..]
                .find("\n\n## ")
                .map(|rel| rel + 1)   // stop before the \n\n separator
                .unwrap_or(tail.len());
            let end = start + block_len;
            self.system.replace_range(start..end, &new_block);
        }
    }
}

fn build_memory_block() -> Option<String> {
    let store = crate::persistence::init().ok()?;
    let entries = store.all_memory().ok()?;

    Some(if entries.is_empty() {
        "## Agent Memory\n\
         No facts saved yet. Use `memory_set` or `/memory set <key> <value>` to persist \
         cross-project facts (e.g. preferred patterns, team conventions, API endpoints) \
         that should be available in future sessions.".to_string()
    } else {
        let facts = entries.iter()
            .map(|(k, v)| format!("- {}: {}", k, v))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "## Agent Memory\n\
             These facts were saved in previous sessions:\n{facts}\n\n\
             You can proactively persist cross-project facts that are worth \
             remembering using `memory_set` or `/memory set <key> <value>`."
        )
    })
}
