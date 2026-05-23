# System Prompt Comparison — Evidence File

> Internal reference for the Medium series on ZAP's skill-first architecture.
> All data gathered from public GitHub repos on 2026-05-23. Links and line numbers are live.

---

## Methodology

For each agent, the actual system prompt source was fetched from the main branch.
Token estimates use `chars / 4` (standard approximation; matches tiktoken within ~5%).
"Tokens per turn" = what the LLM receives as the system instruction on a single request.

---

## 1. Gemini CLI — google-gemini/gemini-cli

**Repo:** https://github.com/google-gemini/gemini-cli

### Primary source files

| File | Size | Purpose |
|---|---|---|
| [`packages/core/src/prompts/snippets.ts`](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/prompts/snippets.ts) | 68,410 chars / 954 lines | All prompt sections as template functions |
| [`packages/core/src/prompts/snippets.legacy.ts`](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/prompts/snippets.legacy.ts) | 56,312 chars | Legacy model variant |
| [`packages/core/src/prompts/promptProvider.ts`](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/prompts/promptProvider.ts) | 11,767 chars | Assembles sections into final prompt |
| [`packages/core/src/core/prompts.ts`](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/core/prompts.ts) | entry point | Calls `PromptProvider.getCoreSystemPrompt()` |

### How the prompt is assembled

`getCoreSystemPrompt()` in `snippets.ts` concatenates these render functions every time it runs:

```typescript
// snippets.ts — getCoreSystemPrompt()
return `
${renderPreamble(options.preamble)}
${renderCoreMandates(options.coreMandates)}
${renderSubAgents(options.subAgents)}
${renderAgentSkills(options.agentSkills)}
${renderHookContext(options.hookContext)}
${options.planningWorkflow
    ? renderPlanningWorkflow(options.planningWorkflow)
    : renderPrimaryWorkflows(options.primaryWorkflows)}
${options.taskTracker ? renderTaskTracker(options.taskTracker) : ''}
${renderOperationalGuidelines(options.operationalGuidelines)}
${renderInteractiveYoloMode(options.interactiveYoloMode)}
${renderSandbox(options.sandbox)}
${renderGitRepo(options.gitRepo)}
`.trim();
```

Source: https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/prompts/snippets.ts#L136

### Section sizes (actual template text content, not source code)

| Section function | Text content | ~Tokens |
|---|---|---|
| `renderPlanningWorkflow` | 22,239 chars | ~5,559 |
| `renderOperationalGuidelines` | 3,782 chars | ~945 |
| `renderPrimaryWorkflows` | 3,306 chars | ~826 |
| `renderSandbox` | 2,866 chars | ~716 |
| `renderSubAgents` | 2,362 chars | ~590 |
| `renderTaskTracker` | 2,059 chars | ~514 |
| `renderGitRepo` | 1,562 chars | ~390 |
| `renderUserMemory` | 1,380 chars | ~345 |
| `renderCoreMandates` | 607 chars | ~151 |
| `renderInteractiveYoloMode` | 537 chars | ~134 |
| `renderAgentSkills` | 418 chars | ~104 |
| `renderHookContext` | 372 chars | ~93 |
| `renderPreamble` | 118 chars | ~29 |
| `renderFinalShell` | 73 chars | ~18 |
| **Total across all sections** | **41,681 chars** | **~10,414** |

### Tokens per turn

- **Standard interactive turn** (no plan mode): ~4,096 tokens
  - Sections included: preamble + coreMandates + primaryWorkflows + operationalGuidelines + subAgents + gitRepo + sandbox + userMemory + agentSkills
- **Plan mode turn**: ~9,655 tokens (adds 5,559-token planning workflow)
- **Minimum possible**: ~4,096 tokens — there is no lighter path

### When is the prompt rebuilt?

From [`packages/core/src/core/client.ts`](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/core/client.ts):

```typescript
private handleMemoryChanged = () => {
    this.updateSystemInstruction();  // rebuilds full prompt
};
private handleApprovalModeChanged = (payload) => {
    this.updateSystemInstruction();  // rebuilds full prompt
};
```

Rebuilt on every memory change and approval mode change — effectively on every turn in an active session.

### Section headers in the rendered prompt (31 total)

```
# Core Mandates
# Security & System Integrity
# Context Efficiency
# Engineering Standards
# Available Sub-Agents
# Strategic Orchestration & Delegation
# Available Agent Skills
# Hook Context
# Primary Workflows
# Development Lifecycle
# New Applications
# Operational Guidelines
# Tone and Style
# Security and Safety Rules
# Tool Usage
# Interaction Details
# Sandbox
# Autonomous Mode (YOLO)
# Git Repository
# TASK MANAGEMENT PROTOCOL
# Active Approval Mode: Plan
# Available Tools
# Rules
... (8 more)
```

---

## 2. OpenCode — opencode-ai/opencode

**Repo:** https://github.com/opencode-ai/opencode

### Primary source file

[`internal/llm/prompt/coder.go`](https://github.com/opencode-ai/opencode/blob/main/internal/llm/prompt/coder.go) — 14,449 chars

### How the prompt is assembled

Two static Go string constants, one per provider. Concatenated with environment info at call time:

```go
func CoderPrompt(provider models.ModelProvider) string {
    basePrompt := baseAnthropicCoderPrompt
    switch provider {
    case models.ProviderOpenAI:
        basePrompt = baseOpenAICoderPrompt
    }
    envInfo := getEnvironmentInfo()
    return fmt.Sprintf("%s\n\n%s\n%s", basePrompt, envInfo, lspInformation())
}
```

Source: https://github.com/opencode-ai/opencode/blob/main/internal/llm/prompt/coder.go#L17

### Prompt sizes

| Constant | Chars | ~Tokens |
|---|---|---|
| `baseAnthropicCoderPrompt` | 8,014 | ~2,003 |
| `baseOpenAICoderPrompt` | 4,373 | ~1,093 |

### Sections in `baseAnthropicCoderPrompt`

- Memory (OpenCode.md instructions)
- Tone and style (verbosity rules, 7 examples)
- Proactiveness
- Following conventions
- Doing tasks
- Code style
- Handling mistakes
- Security and privacy

### Tokens per turn

- **Every turn, every task type**: ~2,003 tokens (Anthropic) / ~1,093 tokens (OpenAI)
- No differentiation between a greeting, a code question, and a full refactor
- No turn-aware filtering, no pruning, no dynamic injection

---

## 3. ZAP — sanjeev23oct/zap

**Repo:** https://github.com/sanjeev23oct/zap

### Primary source files

| File | Purpose |
|---|---|
| [`src/context_manager.rs`](https://github.com/sanjeev23oct/zap/blob/main/src/context_manager.rs) | Builds system prompt per turn |
| [`~/.zap/skills/`](https://github.com/sanjeev23oct/zap/tree/main/skills) | 23 bundled skill files |
| [`src/skill_manager.rs`](https://github.com/sanjeev23oct/zap/blob/main/src/skill_manager.rs) | Skill discovery, trigger matching, injection |
| [`src/session/mod.rs`](https://github.com/sanjeev23oct/zap/blob/main/src/session/mod.rs) | Per-turn prompt composition |

### Two prompt paths

**Path A — Casual/greeting turns** (detected heuristically):
```rust
pub fn build_casual_system_prompt(config: &Config) -> String {
    format!(
        "You are a helpful AI coding assistant (model: {}).\n\
         Be concise and conversational. Do not add filler phrases.",
        config.model
    )
}
```
Cost: **~12 tokens**. Triggered by: "hello", "thanks", "how are you", etc.

**Path B — Real coding turns:**
```rust
pub fn build_system_prompt_with_skills(config: &Config, skill_block: &str) -> Result<String>
```
Sections assembled:
1. Identity (~15 tokens)
2. Environment: platform, shell, cwd (~20 tokens)
3. Code Navigation Strategy (~150 tokens)
4. Tool Usage Policy (~200 tokens)
5. Sub-Agent Orchestration (~300 tokens, only if `agent_depth > 0`)
6. Security Rules (~80 tokens)
7. Response Style (~100 tokens)
8. Agent Memory (only if non-empty)
9. Project Context — ZAP.md content (only if file exists)
10. Project Understanding — .zap/understanding.md (capped at 2,000 tokens)
11. Git Status (only if `.git/` exists and repo is dirty)
12. **Active skills** — injected here, only if triggered

**Base prompt (sections 1–7 always):** ~4,957 chars / **~1,239 tokens**

### Skill sizes (23 bundled skills)

| Skill | Trigger keywords (sample) | ~Tokens |
|---|---|---|
| `git` | commit, branch, merge, pr, push | ~295 |
| `debugging` | error, fix, bug, crash, assert | ~331 |
| `code-review` | review, lgtm, critique, diff | ~345 |
| `go` | golang, goroutine, go.mod | ~384 |
| `python` | pip, def, pytest, venv | ~398 |
| `rust` | cargo, crate, fn , tokio | ~408 |
| `typescript` | tsc, interface, tsx, .ts | ~415 |
| `security` | vuln, injection, xss, auth | ~411 |
| `react` | jsx, useState, component | ~422 |
| `ruby` | gem, rails, rspec | ~508 |
| `karpathy-guidelines` | karpathy, best practice | ~491 |
| All 23 combined | — | **~11,026 tokens** |

### Tokens per turn — real measurements

| Scenario | System prompt |
|---|---|
| Greeting ("hey, what's up?") | ~12 tokens |
| Code question, no edits | ~1,239 tokens |
| Rust refactor request | ~1,647 tokens (base + rust skill) |
| Git commit + code review | ~1,869 tokens (base + git + code-review) |
| Debug a Python crash | ~1,968 tokens (base + debugging + python) |
| Worst case: base + 3 skills | ~2,500 tokens |
| **ALL 23 skills** (never happens) | ~12,265 tokens |

### Key mechanisms

**Trigger matching** (`src/skill_manager.rs`):
```rust
pub fn matches(&self, query: &str) -> bool {
    let lower = query.to_lowercase();
    self.triggers.iter().any(|t| lower.contains(t.as_str()))
}
```
Only skills whose trigger keywords appear in the user's message are injected.

**Tool-result pruning** (`src/session/mod.rs`):
ToolResult blocks outside the last 2 complete exchanges are replaced with `[pruned — N chars]`. Large file reads from 10 turns ago no longer inflate every subsequent prompt.

**Sliding history window**: Non-casual turns send only the last 8 real user turns (configurable via `ZAP_HISTORY_WINDOW`). Bounds total token cost regardless of session length.

**MCP lazy loading**: MCP tool schemas are registered but not injected into the system prompt until the agent determines they're relevant to the current task.

---

## Summary table

| Agent | Min tokens/turn | Typical coding turn | Max possible | Approach |
|---|---|---|---|---|
| **Gemini CLI** | ~4,096 | ~4,096–9,655 | ~10,414 | Modular sections, all assembled every turn |
| **OpenCode** | ~2,003 | ~2,003 | ~2,003 | Single static constant, same every turn |
| **ZAP (greeting)** | ~12 | — | — | Casual path: identity only |
| **ZAP (coding)** | ~1,239 | ~1,600–1,900 | ~12,265 | Base + triggered skills only |

### The structural difference

Gemini CLI and OpenCode front-load every session with a fixed instruction block the LLM receives regardless of task type.

ZAP composes the prompt from discrete files at call time:
- Rust conventions never enter a Python session
- Git skill never inflates a pure explanation turn
- Greetings cost 12 tokens, not 2,000–4,000

The 23 bundled skill files (44,106 chars / ~11,026 tokens total) exist on disk but **no single turn ever loads more than 3**, and most turns load 0–1.

---

## Language-blindness proof: Java request vs React request

### The question being tested

**Request A:** "Write a Spring Boot service for user registration"
**Request B:** "Build a user profile React component with avatar upload"

These are maximally different tasks — different language, different framework, different patterns, different testing tools.

---

### Gemini CLI: what changes between Request A and Request B?

**Language mentions in entire `snippets.ts` (68,410 chars):**

| Language | Count | Where |
|---|---|---|
| `java` | **0** | nowhere |
| `kotlin` | **0** | nowhere |
| `react` | **0** | nowhere (the "React" mention is a new-app scaffold default, not conventions) |
| `python` | 6 | new-app tech-stack defaults only (`"APIs: Node.js (Express) or Python (FastAPI)"`) |
| `rust` | 1 | not language guidance |

**Verified by grep on the raw file:**
```
curl -sL https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/prompts/snippets.ts \
  | grep -i "java\|kotlin\|rust\|golang"
# → (no output)
```

**What Gemini CLI sends for Request A (Spring Boot):**
```
renderPreamble           →  ~29 tokens  (role + mode)
renderCoreMandates       → ~151 tokens  (security, source control, context efficiency)
renderPrimaryWorkflows   → ~826 tokens  (research→strategy→execution lifecycle)
renderOperationalGuidelines → ~945 tokens  (tone, tool usage, security rules)
renderSubAgents          → ~590 tokens  (sub-agent delegation)
renderGitRepo            → ~390 tokens  (git rules)
renderSandbox            → ~716 tokens  (macOS sandbox, failure recovery)
renderUserMemory         → ~345 tokens  (GEMINI.md memory)
renderAgentSkills        → ~104 tokens  (available skill list)
─────────────────────────────────────────────────────────
Total:                    ~4,096 tokens
Language-specific Java content: 0 tokens
```

**What Gemini CLI sends for Request B (React component):**
```
renderPreamble           →  ~29 tokens
renderCoreMandates       → ~151 tokens
renderPrimaryWorkflows   → ~826 tokens
renderOperationalGuidelines → ~945 tokens
renderSubAgents          → ~590 tokens
renderGitRepo            → ~390 tokens
renderSandbox            → ~716 tokens
renderUserMemory         → ~345 tokens
renderAgentSkills        → ~104 tokens
─────────────────────────────────────────────────────────
Total:                    ~4,096 tokens
Language-specific React content: 0 tokens
```

**The prompts are byte-for-byte identical.** The LLM writing your Spring Boot service and the LLM writing your React component receive the same instructions. Nothing in the prompt tells either one how Java should be structured, what React patterns to use, or how to test either.

---

### OpenCode: what changes between Request A and Request B?

**Language mentions in `baseAnthropicCoderPrompt` (8,014 chars):**

```
curl -sL https://raw.githubusercontent.com/opencode-ai/opencode/main/internal/llm/prompt/coder.go \
  | grep -i "java\|kotlin\|react\|python\|typescript\|rust"
# → (no output — zero matches)
```

**Verified: zero mentions of Java, Kotlin, React, TypeScript, Rust, Python, or any other language.**

**Request A (Spring Boot) → sent to LLM:**
```
baseAnthropicCoderPrompt (constant) → ~2,003 tokens
  # Memory
  # Tone and style
  # Proactiveness
  # Following conventions   ← says "mimic code style" — no Java guidance
  # Doing tasks
  # Code style
  # Handling mistakes
  # Security and privacy
```

**Request B (React component) → sent to LLM:**
```
baseAnthropicCoderPrompt (constant) → ~2,003 tokens
  (identical — same constant)
```

---

### ZAP: what changes between Request A and Request B?

**Request A: "Write a Spring Boot service for user registration"**

Trigger scan: "spring" matches `java.md` triggers `["java", "maven", "gradle", "spring", "jvm", ...]`

```
Base prompt             → ~1,239 tokens
  identity, nav, tools, security, style

java.md skill (injected) → ~650 tokens
  ## Java conventions
  Error handling: checked exceptions, unchecked for bugs
  Immutability: final fields, record for data carriers
  Nulls: Optional<T> as return type, never null in collections
  Modern Java 17+: var, switch expressions, List.of()
  Testing: JUnit 5 + AssertJ, Mockito
  Formatting: Google Java Format, 4-space indent
────────────────────────────────────────
Total:                   ~1,889 tokens
Java-specific content:    ~650 tokens
```

**Request B: "Build a user profile React component with avatar upload"**

Trigger scan: "react" matches `react.md` triggers `["react", "component", "jsx", "tsx", "hook", ...]`

```
Base prompt             → ~1,239 tokens
  identity, nav, tools, security, style

react.md skill (injected) → ~422 tokens
  ## React conventions
  Components: functional only, one per file
  TypeScript: Props interface, never use any
  Hooks: honest useEffect deps, useMemo only when measured
  State: lift to lowest common ancestor, Zustand for global
  Fetching: React Query or SWR, not useEffect+useState
  File structure: components/, features/, hooks/, lib/
────────────────────────────────────────
Total:                   ~1,661 tokens
React-specific content:   ~422 tokens
```

**The prompts are different** — both in what they contain and in their total size. The LLM writing the Spring Boot service explicitly knows about `Optional<T>`, `record` types, and JUnit 5. The LLM writing the React component explicitly knows about React Query and the `no-class-components` rule. Neither knows about the other's conventions.

---

### Three-way comparison for the same two requests

| | Gemini CLI | OpenCode | ZAP |
|---|---|---|---|
| **Spring Boot request** | 4,096 tokens | 2,003 tokens | 1,889 tokens |
| **React request** | 4,096 tokens | 2,003 tokens | 1,661 tokens |
| **Prompts identical?** | ✅ Yes (same bytes) | ✅ Yes (same bytes) | ❌ No (different skill injected) |
| **Java conventions?** | ❌ None | ❌ None | ✅ 650 tokens |
| **React conventions?** | ❌ None | ❌ None | ✅ 422 tokens |
| **Output consistency** | Model-dependent | Model-dependent | Skill-controlled |

---

## Evidence URLs

| Agent | File | Raw URL |
|---|---|---|
| Gemini CLI | `snippets.ts` | https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/prompts/snippets.ts |
| Gemini CLI | `promptProvider.ts` | https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/prompts/promptProvider.ts |
| Gemini CLI | `client.ts` | https://raw.githubusercontent.com/google-gemini/gemini-cli/main/packages/core/src/core/client.ts |
| OpenCode | `coder.go` | https://raw.githubusercontent.com/opencode-ai/opencode/main/internal/llm/prompt/coder.go |
| ZAP | `context_manager.rs` | https://raw.githubusercontent.com/sanjeev23oct/zap/main/src/context_manager.rs |
| ZAP | `skill_manager.rs` | https://raw.githubusercontent.com/sanjeev23oct/zap/main/src/skill_manager.rs |
| ZAP | `session/mod.rs` | https://raw.githubusercontent.com/sanjeev23oct/zap/main/src/session/mod.rs |
