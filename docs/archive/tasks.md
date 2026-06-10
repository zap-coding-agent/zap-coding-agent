# Zap — Task Tracker

## Completed ✅
Phases 1–8: Foundation, MVP agent loop, security/permissions, persistence, REPL, streaming, parallel tools,
web fetch/search, diff view, auto-compaction, MCP, sub-agents, sessions, skills loader,
workflow parser, snapshot/undo, provider switching (persistent), banner redesign.

Phase 9 (May 2026):
- ✅ 9.1 Ripgrep search (was already implemented)
- ✅ 9.3 Stack auto-detection (`detect_stack_skills` in skill_manager.rs)
- ✅ 9.4 Secret scrubbing (`secret_scanner.rs`, intercept in agent_core before cloud send)
- ✅ 9.5 Auto skill capture (`/skill capture [name] [--global]`)
- ✅ 9.6 Workflow execution (`/run`, `/workflow new`, step approval gate)
- ✅ 9.7 Cost attribution (per-turn: skills Nt · msg ~Nt · ctx ~Nk · total ~$N)
- ✅ 9.8 Session branching (`/branch`, `/branches`, `/switch`, `/merge`)

---

## Active: Phase 9 — Differentiating Features

Priority rule: **differentiating first, quality-of-output second, polish last.**
Features Goose/Claude Code already have are not here unless they directly unblock a differentiator.

---

### Task 9.1 — Ripgrep-powered search  `[quality]`
**Why:** Agent currently uses `/usr/bin/grep -rn` — misses gitignored files, no structured output, slow on large repos. Fixes a daily-use quality hole.
- [ ] In [tool_registry.rs](src/tool_registry.rs): shell to `rg --json`; fall back to `grep -rn` if `rg` not found
- [ ] New params: `case_insensitive: bool`, `fixed_string: bool`, `file_type: Option<String>`
- [ ] Structured output per match: `{file, line, match_text, context_before, context_after}`
- [ ] `.gitignore` respected by default; `include_ignored: bool` override
- [ ] `cargo check` + manual test: `zap --goal "find all TODO comments in src/"`

**Effort:** 1 day

---

### Task 9.2 — Code map tool (tree-sitter)  `[quality + differentiating]`
**Why:** Agent reads entire files to find a function. A structural outline (functions, structs, line numbers) cuts context usage 5–10x in large repos. Nothing in Goose today.
- [ ] Add to [Cargo.toml](Cargo.toml): `tree-sitter = "0.24"`, grammars for Rust + Python + TypeScript + Go
- [ ] [NEW src/code_index.rs](src/code_index.rs): `build_file_map(path) -> Vec<SymbolDef>` (name, kind, line)
- [ ] [NEW src/code_index.rs](src/code_index.rs): `build_dir_map(path, depth) -> DirMap` for directory overview
- [ ] [tool_registry.rs](src/tool_registry.rs): register `CodeMapTool` with params `path`, `max_depth`
- [ ] Output format: `fn handle_user_turn (line 168)`, `struct Session (line 80)` — compact, usable
- [ ] Manual test: run `code_map` on `src/agent_core.rs`, verify all public fns appear with correct lines

**Effort:** 3–4 days

---

### Task 9.3 — Stack auto-detection  `[differentiating]`
**Why:** Zero-config onboarding. On session start, detect tech stack from project files and auto-activate matching skills — user does nothing. No other agent tool does this.
- [ ] [skill_manager.rs](src/skill_manager.rs): `fn detect_stack_skills(skills: &[Skill]) -> Vec<&Skill>`
  - `Cargo.toml` → look for skill named `rust` or tagged `rust`
  - `package.json` → `typescript` / `node`
  - `pyproject.toml` / `setup.py` → `python`
  - `go.mod` → `go`
- [ ] [agent_core.rs](src/agent_core.rs): call at `Session::new`, merge with user's skills list
- [ ] Print on startup: `  ◎ auto-skills: rust (stack detected)` — only if something matched
- [ ] No crash if no skills directory exists

**Effort:** 0.5 days

---

### Task 9.4 — Secret scrubbing before cloud send  `[differentiating + trust]`
**Why:** No other agent tool does this. One leaked API key destroys trust permanently. Scan file content for secrets before sending to Anthropic/DeepSeek/OpenAI. Skip for local LM Studio.
- [ ] [NEW src/secret_scanner.rs](src/secret_scanner.rs): `fn scan(content: &str) -> Vec<SecretMatch>`
  - Patterns: `sk-ant-`, `sk-`, `ghp_`, `ghs_`, `-----BEGIN`, `api_key\s*=`, `password\s*=`, AWS `AKIA`
  - Return: `SecretMatch { pattern, line, preview }` (preview = first 20 chars, rest `***`)
- [ ] [tool_registry.rs](src/tool_registry.rs): call scanner on `read_file` and `edit_file` results when `config.provider != LM Studio`
- [ ] [agent_core.rs](src/agent_core.rs): if matches found, print warning and prompt `send anyway? [y/N]`; default N
- [ ] Add `secret_scanning: bool` to `Config` (default true, can disable in `~/.agent.toml`)

**Effort:** 1 day

---

### Task 9.5 — Auto skill capture (`/skill capture`)  `[differentiating]`
**Why:** Turns one-time user corrections into permanent reusable skills. Unique to zap — sessions become team knowledge assets. No equivalent in Goose or Claude Code.

How it works: user runs `/skill capture my-rules` → zap sends the session to the LLM asking it to extract instructions/preferences → saves as a skill file.
- [ ] [skill_manager.rs](src/skill_manager.rs): `fn save_captured_skill(name: &str, content: &str, global: bool) -> Result<PathBuf>`
  - `global=false` → `.zap/skills/<name>.md` (project-local)
  - `global=true` → `~/.zap/skills/<name>.md`
- [ ] [agent_core.rs](src/agent_core.rs): handle `/skill capture [name] [--global]`
  - Build a prompt from last N messages asking LLM to extract instructions
  - Call LLM (single non-streaming call), save result
  - Print: `  ✓ skill saved → .zap/skills/my-rules.md  (activate with: /skill list)`
- [ ] Guard: refuse if fewer than 3 turns (nothing meaningful to capture)

**Effort:** 1 day

---

### Task 9.6 — Workflow execution  `[differentiating]`
**Why:** `workflow.rs` already parses `.zap/workflows/*.yaml`. The execution engine is missing. Workflows are a team primitive — checked into repos, versioned, shared. Nothing like it in Goose.
- [ ] [workflow.rs](src/workflow.rs): `pub async fn run_workflow(name: &str, session: &mut Session) -> Result<()>`
  - Iterate steps sequentially
  - Per step: inject `step.skill` if set, run `step.prompt` as a user turn
  - If `requires_approval: true`: print step summary, wait for `[Enter] to continue / q to abort`
  - Print step progress: `  [1/3] code-review …` / `  [2/3] test-runner …`
- [ ] [agent_core.rs](src/agent_core.rs): `/run <workflow>` slash command; list available on `/run` alone
- [ ] [cli.rs](src/cli.rs): `--workflow <name>` flag for headless/CI use
- [ ] [agent_core.rs](src/agent_core.rs): add `/workflow new <name>` to scaffold a workflow file
- [ ] Manual test: create a 2-step workflow, run it, verify step injection and approval gate

**Effort:** 2 days

---

### Task 9.7 — Token cost attribution per component  `[differentiating]`
**Why:** Makes the "token efficiency" story visible and verifiable. Users can see exactly what skills/tools cost. No other tool shows this breakdown. Directly proves the value prop.

After each turn, print:
```
  ↳ rust-expert: 820t  context: 1.2k  msg: 45t  tools: 380t  │  total: 2.4k  ~$0.0032
```
- [ ] [agent_core.rs](src/agent_core.rs): track `skill_tokens` (sum of matched skill body lengths / 4), `tool_result_tokens` (sum of tool result lengths / 4)
- [ ] After each turn, print attribution line using existing `format_cost()` + new component breakdown
- [ ] Only show skill tokens if skills were matched that turn
- [ ] Keep it on one line; dim the component labels, bright the numbers

**Effort:** 1 day

---

### Task 9.8 — Session branching  `[differentiating]`
**Why:** Unique primitive — nothing like it in any coding agent. Fork a conversation, try approach B, return to A. Git-like experimentation for multi-turn sessions.

Commands: `/branch <name>`, `/branches`, `/switch <name>`, `/merge <name>`
- [ ] [persistence.rs](src/persistence.rs): new table `branches (id, session_id, name, parent_name, messages_json, created_at)`
- [ ] [persistence.rs](src/persistence.rs): `save_branch`, `list_branches`, `load_branch`, `delete_branch`
- [ ] [agent_core.rs](src/agent_core.rs): wire slash commands; show active branch in prompt when not `main`: `[3:try-rewrite] ›`
- [ ] `/branch <name>` — snapshot current `self.messages` into named branch, continue on new branch
- [ ] `/switch <name>` — swap `self.messages` to named branch's snapshot
- [ ] `/merge <name>` — ask LLM for a 3-sentence summary of the branch, append as assistant message to current
- [ ] `/branches` — list all branches for current session with turn count

**Effort:** 2–3 days

---

## Phase 10 — Infrastructure (unblocks differentiators)

These are not unique but are needed for quality output. Do after Phase 9.

### Task 10.1 — Prompt caching (Anthropic)  `[~free cost savings]`
- [ ] [llm_client.rs](src/llm_client.rs): add `cache_control: {"type": "ephemeral"}` breakpoints to system prompt and tool definitions when `provider == Anthropic`
- [ ] Verify in Anthropic dashboard: `cache_read_input_tokens` appears on repeated turns

**Effort:** 0.5 days

---

### Task 10.2 — Per-session permission memory  `[UX]`
- [ ] [permission_manager.rs](src/permission_manager.rs): add `session_grants: HashMap<String, bool>`
- [ ] After first approval of a tool class, skip re-prompting for that class this session
- [ ] `/permissions reset` clears grants; show active grants in `/config`

**Effort:** 0.5 days

---

### Task 10.3 — Token budget flag  `[pairs with 9.7]`
- [ ] [cli.rs](src/cli.rs): add `--budget N` (token count)
- [ ] [agent_core.rs](src/agent_core.rs): warn at 80% of budget; hard-stop at 100% with clear message

**Effort:** 0.5 days

---

### Task 10.4 — Multi-edit tool  `[agent reliability]`
- [ ] [tool_registry.rs](src/tool_registry.rs): `BatchEditTool` — `path` + `edits: [{old_string, new_string}]`; apply sequentially, single diff at end

**Effort:** 0.5 days

---

## Phase 11 — Advanced Differentiators (after Phase 9 ships)

### Task 11.1 — Semantic skill routing (local embeddings)
**Why:** Skill keyword matching misses intent. Replace with local embedding model (fastembed, ONNX, no cloud). Skills fire on meaning.
- [ ] [Cargo.toml](Cargo.toml): `fastembed = "4"`
- [ ] [skill_manager.rs](src/skill_manager.rs): precompute skill embeddings at startup; `match_skills_semantic` with cosine similarity fallback
- [ ] First-run downloads model (~90MB) to `~/.cache/zap/`; keyword fallback if model unavailable

**Effort:** 2 days

---

### Task 11.2 — Multi-model routing
**Why:** Use cheap/fast model for tool calls, expensive model for generation. 10x cost reduction on tool-heavy sessions.
- [ ] [NEW src/router.rs](src/router.rs): load `~/.zap/routing.toml`, `fn select_model(query) -> ModelConfig`
- [ ] [agent_core.rs](src/agent_core.rs): swap model per turn, show selected model in attribution line

**Effort:** 2 days

---

## Build order summary

```
Week 1 (differentiating + quality):
  9.1 ripgrep  →  9.3 stack-detect  →  9.4 secret-scrub  →  9.7 cost-attribution

Week 2 (differentiating features):
  9.5 skill-capture  →  9.6 workflow-execution  →  9.2 code-map (start)

Week 3:
  9.2 code-map (finish)  →  9.8 session-branching  →  10.1–10.4 infrastructure

Week 4+:
  11.1 semantic routing  →  11.2 multi-model routing
```

---

## Definition of done (every task)
1. `cargo check` passes with no new warnings
2. Feature tested manually against a real prompt
3. Task checkbox ticked here
