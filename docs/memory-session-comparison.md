# Memory & Session Persistence: Zap vs Claude Code

> Sourced from zap source code: `src/persistence.rs`, `src/project.rs`,
> `src/context_manager.rs`, `src/tools/mod.rs`, `src/session/mod.rs`,
> `src/session/turn.rs`, `src/session/commands/memory.rs`.
> Claude Code behavior: observed directly (this document was written running inside Claude Code).

---

## 1. Zap's Storage Systems — Exact Implementation

### A. `~/.zap/agent.db` (SQLite, global across all projects)

Four tables:

| Table | Columns | Purpose |
|---|---|---|
| `sessions` | id, goal, model, created_at | One row per session — metadata only |
| `session_messages` | session_id (PK), content (full JSON blob), updated_at | Full message array per session — UPSERT on conflict |
| `memory` | key (PK), value, updated_at | Global key-value facts, cross-session, cross-project |
| `branches` | id, session_id, name, parent_name, messages_json, turn_count, created_at | Named conversation forks |

**Accessing memory without zap:**
```bash
sqlite3 ~/.zap/agent.db "SELECT key, value, updated_at FROM memory ORDER BY key;"
sqlite3 ~/.zap/agent.db "SELECT id, goal, model, created_at FROM sessions ORDER BY id DESC LIMIT 20;"
```
Power users can query, modify, or export any of this with standard SQLite tools. Zap itself exposes `/memory list|get|set|del` for the same operations.

---

### B. `.zap/context.md` (project-local markdown, session handoff)

Written automatically at session end. Content:
```
# Session Context
## Last updated
2026-06-05 14:30 — Session #271
## What was being worked on
<goal from first user message>
## Files touched
  - src/session/summarizer.rs
  - src/session/mod.rs
## What's next
<LLM-generated or preserved from previous write>
```

**Injection behaviour (verified from `src/session/mod.rs:226–249`):**
- **TUI mode**: always appended to `self.system` at session start as `## Last Session Handoff`
- **CLI mode**: shown as startup banner; user is prompted "Resume from last session?" — only appended to system if they say yes
- **Not re-injected per turn** — only at session start

---

### C. `.zap/session_log.md` (project-local, append-only)

One entry per session, newest first, capped at 20,000 chars:
```
## Session #271 — 2026-06-05
Goal: memory session comparison doc
Files: docs/memory-session-comparison.md
```

**Injection:** **lazy hint only** — the system prompt tells the LLM the file exists and to `read_file` it when the user asks about past work. The content is NOT always in context.

---

### D. `.zap/understanding.md` (project-local, LLM-generated)

Written by `/init`. Sections: `## Analysis`, `## Architecture`, `## Overview`.

**Injection:** injected into `self.system` every session start, **capped at 4,000 chars (~1,000 tokens)**. Only injected if the file contains at least one of the real section headers above — a stub or empty file is skipped.

---

### E. In-memory file snapshots (`src/snapshot.rs`)

Before every `edit_file` or `write_file`, the prior content is saved to a static in-memory stack. Undo restores from it.

**Not persisted** — lost when zap exits. Session-scoped only.

---

### F. System prompt build timing (critical detail)

`self.system` is built **once at session start** (`src/session/mod.rs:81`). It includes the full memory dump from `agent.db` at that moment.

Per-turn dispatch (`src/session/turn.rs`):
- Casual turn → `build_casual_system_prompt()` (~15 tokens, no memory)
- Non-casual, no skill match → `self.system.clone()`
- Non-casual, skill match → `format!("{}\n\n{}", self.system, skill_block)`

**Consequence:** memory entries written with `/memory set` mid-session are persisted to the DB immediately but are **not visible in the current session's system prompt** — they appear on the next session start.

---

## 2. Claude Code's Equivalent Systems

### A. Typed memory files (`~/.claude/projects/<hash>/memory/`)

Each memory is a separate `.md` file with YAML frontmatter:
```markdown
---
name: bundled-pr-preference
description: user prefers one bundled PR over many small ones for refactors
metadata:
  type: feedback
---
For refactors in this area, user prefers one bundled PR over many small ones.
Confirmed after I chose this approach — a validated judgment call, not a correction.

**Why:** splitting refactor PRs was churn in this codebase.
**How to apply:** when scoping refactor work, default to one PR unless told otherwise.
```

Four types: `user` (who they are), `feedback` (how to work), `project` (ongoing context), `reference` (where to find things).

`MEMORY.md` is the index file — one line per memory, always loaded into context at conversation start.

### B. Auto-proactive saving

Claude actively saves memories during conversation **without being asked**, mid-turn, when it observes something worth keeping:
- "user prefers bundled PRs" → saves immediately as a `feedback` memory
- "user is a data scientist" → saves as a `user` memory
- "pipeline bugs tracked in Linear INGEST" → saves as a `reference` memory

This is the LLM calling a file-write operation directly, not instructing the user to run a command.

### C. Conversation storage

One `.jsonl` file per conversation (e.g. `100bda06-fa35-4e83-88b5-aa5b8baf0a86.jsonl`). Readable as raw JSON lines.

### D. `CLAUDE.md` discovery

Recursive upward (to git root) AND downward into subdirectories. `~/.claude/CLAUDE.md` global layer.

### E. No equivalent of

- Conversation branching
- Code symbol index (`code.db`)
- `understanding.md` (LLM-generated architectural knowledge)
- Session log with per-session file tracking
- Sliding window with LLM summarization
- Context percentage viewer (TUI)

---

## 3. Feature-by-Feature Comparison

| Feature | Zap | Claude Code |
|---|---|---|
| **Session persistence** | SQLite `sessions` + `session_messages` tables | `.jsonl` file per conversation |
| **Session resume** | `/sessions` interactive picker | Conversation history in UI |
| **Conversation branching** | ✅ `branches` table (named forks) | ❌ Not available |
| **Memory storage** | SQLite `memory` table (key-value) | Typed `.md` files in project dir |
| **Memory human-readable without tools** | Via `sqlite3` CLI or `/memory list` | ✅ Plain markdown, any editor |
| **Memory types** | ❌ Flat key-value only | ✅ user / feedback / project / reference |
| **Memory auto-save by LLM** | ❌ No `memory_set` tool — user must type `/memory set` | ✅ LLM saves proactively mid-conversation |
| **Memory in system prompt** | ✅ All entries injected every non-casual turn | ✅ MEMORY.md index always in context |
| **Memory relevance filtering** | ❌ All entries injected always | ❌ All entries loaded (index truncated at 200 lines) |
| **Memory descriptions** | ❌ Raw key=value | ✅ One-line description per entry |
| **Memory stale-detection guidance** | ❌ None | ✅ Prompt instructs: verify file paths before acting |
| **New memory visible this session** | ❌ Next session only | ✅ Claude knows what it wrote immediately |
| **Session handoff file** | ✅ `.zap/context.md` (goal + files + what's next) | ❌ None |
| **Session log with file tracking** | ✅ `.zap/session_log.md` | ❌ None |
| **LLM-generated project knowledge** | ✅ `.zap/understanding.md` (from `/init`) | ❌ None |
| **Code symbol index** | ✅ `.zap/code.db` (SQLite, instant lookup) | ❌ Relies on grep/LSP every query |
| **File undo** | ✅ In-memory snapshot stack | ❌ None |
| **Audit log** | ✅ `~/.zap/audit.jsonl` (every tool call) | ❌ No local audit |
| **Sliding window summarization** | ✅ LLM summarizes dropped turns automatically | ❌ Manual `/compact` only |
| **Context viewer** | ✅ TUI overlay with token usage per turn | ❌ None |
| **CLAUDE.md/ZAP.md discovery** | ✅ Up to git root | ✅ Recursive up AND into subdirs |
| **Subdirectory CLAUDE.md** | ❌ Not discovered | ✅ Recursive into packages/ etc. |
| **Casual turn optimization** | ✅ ~15 tokens for "ok"/"yes"/"thanks" | ❌ Full prompt every turn |

---

## 4. The Actual Gaps

### Real gap 1: No `memory_set` tool (the auto-save gap)

This is the only substantive behavioral gap. In Claude Code, when the LLM observes "user prefers bundled PRs", it writes a memory file directly — a tool call with no user involvement. In zap:

- There is **no `memory_set` tool** in `ToolRegistry::new()` (verified: `src/tools/mod.rs:57–83`)
- The system prompt tells the LLM it *can* use `/memory set key value` but this is a CLI slash command, not a tool call
- The LLM would have to output text like "you should run `/memory set ...`" and the user has to type it

**Fix:** add a `MemorySetTool` and `MemoryDeleteTool` to `src/tools/`. Roughly 40 lines each, identical pattern to `TodoWriteTool`.

---

### Real gap 2: New memory not visible mid-session

If a user runs `/memory set` mid-session (or if the LLM had a `memory_set` tool), the DB is updated immediately but `self.system` is not rebuilt — the new fact won't appear in the LLM's context until next session.

**Fix:** when memory changes, rebuild `self.system` via `context_manager::build_system_prompt()`. Or maintain a `session_memory_patch` string appended to each turn until next full rebuild.

---

### Minor gap: flat key-value vs typed memories

Claude Code's 4 types (user/feedback/project/reference) exist to help the agent decide *how* to use a fact and *when* it's still valid. In practice, for the number of memory entries a single project accumulates, flat key-value works fine — the LLM sees everything and can reason about it.

This gap matters only if memory grows large (100+ entries) where filtering by type becomes useful. Not a current problem.

---

### Minor gap: no stale-memory guidance

Claude Code's system prompt explicitly tells it: "verify file paths and function names in memory before acting — they may not exist anymore." Zap's system prompt has no equivalent warning. For large projects with significant refactoring, stale keys can mislead.

---

## 5. Where Zap Beats Claude Code

These are not gaps in zap — they are features Claude Code lacks entirely:

| Feature | Why it matters |
|---|---|
| **Conversation branching** | Try alternative approaches without losing current state |
| **Code symbol index** | `find_definition` returns in milliseconds; Claude Code greps every time |
| **`understanding.md`** | LLM-generated architectural reference, always in context — no need to re-explore on every session |
| **Session log with file tracking** | Know which files were touched in which session |
| **Sliding window + LLM summarization** | Context degrades gracefully across long sessions; Claude Code's only option is manual `/compact` |
| **Context viewer** | See exactly what's in context and how full it is; Claude Code has no equivalent |
| **Audit log** | Every tool call recorded locally — useful for debugging and cost tracking |
| **Casual turn optimization** | 15 tokens vs 8,000+ for "ok" — Claude Code pays full price every turn |
| **Skill-based injection** | Git instructions only when doing git; security rules only for security queries |

---

## 6. Summary

The user's hypothesis is accurate: **the SQLite vs markdown distinction is not a real gap** for power users — `sqlite3` queries are as fast and more powerful than grepping markdown files, and `/memory list` works for casual inspection.

The **one real behavioral gap** is the absence of a `memory_set` LLM tool. Everything else in zap's persistence system is either equivalent to or ahead of Claude Code.

The two-line implementation gap:
```rust
// Add to ToolRegistry::new() in src/tools/mod.rs:
r.register(Arc::new(MemorySetTool));
r.register(Arc::new(MemoryDeleteTool));
```

Plus `src/tools/memory.rs` (~80 lines) implementing the `Tool` trait, calling `crate::persistence::init()?.set_memory(key, val)`. A follow-on: rebuild `self.system` after a successful `memory_set` call so the new fact is available immediately without restarting.
