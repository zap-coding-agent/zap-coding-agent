# zap Backlog

Items pending verification before implementation. Test the failure scenario first, then decide.

---

## [B1] Security Rules missing `--no-verify`

**File:** `src/context_manager.rs` — Security Rules section

**Failure scenario:**
User says "commit this but the tests are failing, just push it". The LLM generates:
```
shell: git commit --no-verify -m "fix: ..."
```
Pre-commit hooks (600-line limit, version bump check, FEATURES.md guard) are bypassed.
The broken commit lands on main. No error is shown — it succeeds silently.

**Fix:** Add as rule 7 in Security Rules:
```
7. Never use --no-verify on git commit or git push — pre-commit hooks enforce
   code quality and must not be bypassed.
```

**Why it matters for zap:** zap's own repo enforces strict hooks. An agent developing zap
(or any project with hooks) can silently break the contract.

---

## [B2] `batch_edit` not mentioned in Tool Usage Policy

**File:** `src/context_manager.rs` — Tool Usage Policy section

**Failure scenario:**
User says "rename the `handle_turn` function to `process_turn` across this file".
The function appears in 4 places in the same file. The LLM calls:
```
edit_file: replace "fn handle_turn" → "fn process_turn"
edit_file: replace "handle_turn(&mut self" → "process_turn(&mut self"
edit_file: replace "self.handle_turn()" → "self.process_turn()"
edit_file: replace "// handle_turn docs" → "// process_turn docs"
```
Four sequential round-trips. After edit 1, the file is in a broken intermediate state.
If the LLM reads the file between edits (for the next old_string), it sees partial changes.
With `batch_edit` this is one atomic call — all four replacements applied in order, one diff shown.

**Fix:** Add to Tool Usage Policy under "Editing files":
```
- For multiple edits to the same file, use `batch_edit` instead of sequential
  `edit_file` calls. It applies all replacements atomically and shows one diff.
```

---

## [B3] `find_references` not in Tool Usage Policy

**File:** `src/context_manager.rs` — Tool Usage Policy section

**Failure scenario:**
User says "delete the `legacy_parse` function, it looks unused".
The LLM calls `find_definition` to locate it, then immediately deletes it with `edit_file`.
It misses that `legacy_parse` is called in `src/compat.rs` and `src/tests/parse_test.rs`.
Build breaks. The LLM has to backtrack.

**Fix:** Add to Tool Usage Policy under "Editing files":
```
- Before renaming or deleting any symbol, call `find_references` first to see
  every call site. Skipping this causes silent breakage.
```

**Note:** `find_references` is currently a text/ripgrep search, not a semantic LSP reference.
It will find text matches — good enough for most renames but can have false positives on
short common names (e.g. `id`, `name`). Verify the result list looks reasonable before acting.

---

## [B4] Shell `timeout` param silently ignored

**File:** `src/tools/shell.rs` schema + `src/shell_runner.rs`

**Failure scenario:**
User says "run the full test suite, it takes about 2 minutes".
The LLM calls:
```json
{ "command": "cargo test", "timeout": 120 }
```
The agent enforces 60s regardless. The test run is killed at 60s with a timeout error.
The LLM retries — same result. User sees repeated failures for a command that would pass at 120s.

Additionally the tool description says "Timeout: 30 s" but the runner uses 60s. If the user
reads the tool description to understand the limit, they get the wrong number.

**Fix options (pick one):**
1. Wire the param: pass `input["timeout"]` into `run_command` and thread it through `run_with_timeout`
2. Remove param from schema + fix description to say "60s (fixed)"

Option 2 is lower risk (no behaviour change). Option 1 is more useful but needs a max cap
(e.g. 300s) to prevent the LLM from setting absurd timeouts.

---

## [B5] "ALWAYS `code_map` first on ANY file" too broad

**File:** `src/context_manager.rs` — Code Navigation Strategy section

**Failure scenario:**
User says "what version is this project?".
The LLM calls:
```
code_map: Cargo.toml        ← returns: 1 symbol (package name only, no version)
read_file: Cargo.toml       ← actually reads the version field
```
Two tool calls for a one-line answer. `code_map` on a TOML config returns almost nothing
useful — it's not a source file. Same waste happens for `package.json`, `go.mod`, `.env`,
`Makefile`, `docker-compose.yml`.

**Fix:** Add a carve-out to the Code Navigation Strategy:
```
Exception: small config/manifest files (Cargo.toml, package.json, go.mod, Makefile,
.env, docker-compose.yml) may be read directly with `read_file` — they contain no
symbols worth mapping and a full read costs nothing.
```
