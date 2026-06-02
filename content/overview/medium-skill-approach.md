# Medium — The Skill-First Architecture Post

**Title:** Why I Replaced the System Prompt with Skills

**Subtitle:** Other AI coding agents send the exact same instructions whether you're writing Java, Kotlin, or React. ZAP sends what the task actually needs — and nothing else.

**Suggested tags:** `Rust`, `AI Agents`, `Software Engineering`, `LLM`, `Developer Tools`

**SEO focus:** AI coding agent system prompt, LLM context optimization, skill injection, language-specific AI coding, Gemini CLI system prompt, OpenCode system prompt, coding agent architecture

---

## The Problem Nobody Talks About

Ask Gemini CLI to write a Spring Boot service. Then ask it to write a React component. Then ask it to write a Kotlin coroutine.

The system prompt it sends the LLM is **identical** for all three.

There's no Java section. No Kotlin section. No React section. The agent sends 4,096 tokens of generic instructions — sandbox rules, planning workflows, git conventions, memory protocols — and delegates the question of "how should Java actually be written?" entirely to the model's training data.

That means the output you get depends on which version of Java the model saw most during training. Whether it uses `Optional<T>` or returns nulls. Whether it favours constructor injection or field injection. Whether it uses JUnit 4 or JUnit 5. The model decides, not you.

I built ZAP with a different assumption: **language quality should be explicit, controlled, and versioned, not left to model intuition.**

---

## What Other Agents Actually Send

I pulled the system prompt source from both Gemini CLI and OpenCode directly.

### Gemini CLI

**Source:** [`packages/core/src/prompts/snippets.ts`](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/prompts/snippets.ts) — 954 lines, 68,410 chars

What gets assembled on every interactive turn:

```
renderPreamble                →    ~29 tokens
renderCoreMandates            →   ~151 tokens
renderPrimaryWorkflows        →   ~826 tokens
renderOperationalGuidelines   →   ~945 tokens
renderSubAgents               →   ~590 tokens
renderGitRepo                 →   ~390 tokens
renderSandbox                 →   ~716 tokens
renderUserMemory              →   ~345 tokens
renderAgentSkills             →   ~104 tokens
──────────────────────────────────────────────
Total every turn:                ~4,096 tokens
```

Plan mode adds another 5,559-token planning workflow. The prompt is rebuilt on every turn.

**Language-specific guidance in any of those sections: zero.**

I searched the entire 68,410-char file:

```bash
curl -sL https://raw.githubusercontent.com/google-gemini/gemini-cli/\
main/packages/core/src/prompts/snippets.ts \
  | grep -i "java\|kotlin\|rust\|golang"
# → (no output)
```

Zero matches for Java, Kotlin, Rust, or Go. The six "Python" mentions in the file are scaffolding defaults ("APIs: Node.js or Python") — not conventions. There is no section that tells the LLM how Python should be written.

The closest language guidance is this line from `renderOperationalGuidelines`:

> *"Rigorously adhere to existing workspace conventions, architectural patterns, and style."*

Figure it out from the codebase. The agent trusts the model to know — and hopes your existing code is good enough to infer from.

### OpenCode

**Source:** [`internal/llm/prompt/coder.go`](https://github.com/opencode-ai/opencode/blob/main/internal/llm/prompt/coder.go) — one Go string constant, 8,014 chars / ~2,003 tokens

```bash
curl -sL https://raw.githubusercontent.com/opencode-ai/opencode/\
main/internal/llm/prompt/coder.go \
  | grep -i "java\|kotlin\|react\|python\|typescript\|rust"
# → (no output)
```

**Zero matches.** Not a single mention of any language. The "Following conventions" section says:

> *"When making changes to files, first understand the file's code conventions. Mimic code style, use existing libraries and utilities, and follow existing patterns."*

Same 2,003 tokens for Java, React, Kotlin, Python. Every turn. The language is invisible to the system prompt.

### The concrete proof: Java request vs React request

Take two maximally different tasks:

- **Request A:** "Write a Spring Boot service for user registration"
- **Request B:** "Build a user profile React component with avatar upload"

**Gemini CLI — what changes between A and B:**
```
Request A (Spring Boot):  4,096 tokens — Java-specific content: 0
Request B (React):        4,096 tokens — React-specific content: 0
Diff:                     identical
```

**OpenCode — what changes between A and B:**
```
Request A (Spring Boot):  2,003 tokens — Java-specific content: 0
Request B (React):        2,003 tokens — React-specific content: 0
Diff:                     identical
```

The LLM writing your Spring Boot service and the LLM writing your React component receive the exact same instructions. Zero language signal.

---

## What ZAP Does Instead

ZAP ships with 23 language and practice skill files. Each one is an explicit, opinionated guide for a specific language or domain. They live in `~/.zap/skills/` and are injected into the system prompt **only when you're actually working in that language.**

**ZAP — what changes between Request A and Request B:**
```
Request A (Spring Boot):
  Base prompt:   ~1,239 tokens  (identity, nav, tools, security, style)
  java.md:        ~650 tokens  ← injected because "spring" is a trigger
  Total:         ~1,889 tokens — 650 tokens of explicit Java conventions

Request B (React component):
  Base prompt:   ~1,239 tokens
  react.md:       ~422 tokens  ← injected because "react" is a trigger
  Total:         ~1,661 tokens — 422 tokens of explicit React conventions

Diff: different content, different size, different conventions
```

The prompts are not the same. The LLM writing Spring Boot gets Java conventions. The LLM writing React gets React conventions. Neither gets the other's.

Here's what those skills actually contain — real files, not pseudocode:

```markdown
---
name: java
category: domain
trigger: ["java", "maven", "gradle", "spring", "jvm", ".java",
          "public class", "implements ", "extends ", "junit",
          "lombok", "jakarta", "record ", "interface "]
tokens: 650
---

## Java conventions

**Error handling:** Throw checked exceptions only when callers can
reasonably recover. Prefer unchecked (RuntimeException) for
programming errors. Never swallow exceptions with an empty catch.

**Immutability:** Prefer immutable classes — final fields, no setters.
Use Java 16+ record for pure data carriers. Builder pattern for
mutable builders (Effective Java Item 2).

**Nulls:** Prefer Optional<T> as a return type when absence is expected.
Never pass Optional as a parameter. Avoid null in collections.

**Modern Java (17+):**
- Use var where the type is obvious from the right-hand side.
- Use switch expressions and pattern matching.
- Prefer List.of(), Map.of(), Set.of() for immutable collections.

**Testing:** JUnit 5 + AssertJ for assertions. Mockito for mocking.
Name tests methodName_scenario_expectedResult.

**Formatting:** Google Java Format or Checkstyle. 4-space indent,
100-char line limit.
```

When you type "write a Spring Boot endpoint that handles user registration", the word "Spring" triggers the java skill. The LLM receives 650 tokens of explicit Java conventions. It knows to use `Optional<T>`, JUnit 5, `record` types, and constructor injection — because you told it to, not because it guessed.

When you switch to Kotlin:

```markdown
---
name: kotlin
trigger: ["kotlin", ".kt", "fun ", "data class", "coroutine",
          "kotlinx", "compose", "flow", "suspend fun"]
---

## Kotlin conventions

**Null safety:** Never use !! — it is a crash waiting to happen.
Use ?.let, ?:, requireNotNull with a meaningful message.

**Coroutines:**
- Launch from a CoroutineScope — never GlobalScope in production.
- Use viewModelScope / lifecycleScope in Android.
- Flow for streams, suspend fun for single async values.
- withContext(Dispatchers.IO) for blocking I/O.

**Scope functions:** let for nullable transformation, apply for
object configuration, run for scope with result. Don't chain more
than two.
```

And for React:

```markdown
---
name: react
trigger: ["react", "component", "jsx", "tsx", "hook",
          "usestate", "useeffect", "next.js"]
---

## React conventions

**Components:** Always functional — no class components.
One component per file.

**State:** Lift to lowest common ancestor. useState for local UI,
context for cross-tree, Zustand/Jotai for global app state.

**Fetching:** Use React Query or SWR for server state — not
useEffect + useState for data fetching.
```

These are not inferred from the codebase. They are explicit, versioned, owned instructions that live in files you can read, edit, and audit.

---

## The Token Picture

ZAP's base system prompt (always present on real turns): **~1,239 tokens**

Skills injected on top:

| Skill | Trigger sample | ~Tokens |
|---|---|---|
| `java` | java, spring, gradle, junit | ~650 |
| `kotlin` | kotlin, .kt, coroutine, compose | ~520 |
| `react` | react, jsx, tsx, usestate | ~422 |
| `typescript` | tsc, interface, tsx | ~415 |
| `python` | pip, def, pytest | ~398 |
| `rust` | cargo, crate, tokio | ~408 |
| `go` | golang, goroutine, go.mod | ~384 |
| `debugging` | error, fix, bug, crash | ~331 |
| `git` | commit, branch, merge, pr | ~295 |
| `code-review` | review, lgtm, critique | ~345 |
| All 23 combined | — | **~11,026** |

**What a turn actually sends:**

| What you type | Tokens sent |
|---|---|
| "hey what does this function do?" | ~12 (casual path) |
| "add a retry on this API call" | ~1,239 (base only) |
| "write a Spring Boot registration endpoint" | ~1,889 (base + java) |
| "add coroutine-based caching in Kotlin" | ~1,759 (base + kotlin) |
| "build a user profile React component" | ~1,661 (base + react) |
| "debug why this Python script crashes" | ~1,968 (base + debugging + python) |

For comparison, Gemini CLI and OpenCode send **~4,096** and **~2,003 tokens** respectively for every single one of those turns — with no language-specific guidance in any of them.

---

## Why "Infer from the Codebase" Isn't Enough

The conventional answer to "how does the agent know about Java conventions?" is: it reads the existing code and follows the pattern.

This works when:
- The existing code is high quality
- The existing code is consistent
- There's enough of it to establish a pattern

It breaks when:
- You're starting a new project (no existing code to read)
- The codebase has mixed conventions (legacy + modern Java coexisting)
- The model hasn't seen enough of your specific stack in training

ZAP's skills solve all three cases. They're the floor — the minimum standard the agent brings to every session regardless of what the existing code looks like. If the project has its own conventions in ZAP.md, those layer on top and take priority. But the agent always knows the baseline.

---

## How the Injection Works

The trigger matching is intentionally simple:

```rust
// src/skill_manager.rs
pub fn matches(&self, query: &str) -> bool {
    let lower = query.to_lowercase();
    self.triggers.iter().any(|t| lower.contains(t.as_str()))
}
```

Each skill has a list of trigger keywords in its frontmatter. If any appear in the user's message, the skill is injected. No embeddings, no scoring, no semantic search — substring match at the start of each turn.

```
User: "write a Spring Boot service for user registration"
         ↓
Scan 23 skills for trigger matches
  java.md triggers: ["java", "maven", "spring", ...] → "spring" matches ✓
  kotlin.md triggers: ["kotlin", ".kt", ...] → no match
  react.md triggers: ["react", "jsx", ...] → no match
  ...
         ↓
Inject java skill (~650 tokens) into system prompt
         ↓
Total sent: ~1,889 tokens
```

Three-tier skill priority: bundled (shipped with binary) → global (`~/.zap/skills/`) → project (`.zap/skills/`). Project skills override global which override bundled. You can ship a `java.md` in your repo that replaces the default with your team's specific conventions.

---

## Four More Mechanisms That Work Alongside This

Skills are the most visible part. There are four others.

**Casual path.** ZAP detects greetings, acknowledgements, and casual questions ("thanks", "what's up", "looks good") and routes them to a 12-token system prompt. No language skills, no tool policies, no git status. The LLM receives just enough to identify itself.

**Sliding history window.** Non-casual turns send only the last 8 real user turns. Sessions run for hours without the context window growing unboundedly. Standard agents send the full conversation every time.

**Tool-result pruning.** ToolResult blocks outside the last 2 complete exchanges are replaced with `[pruned — N chars]`. That 800-line file you read on turn 3 doesn't re-inflate every subsequent prompt.

**Lazy MCP loading.** MCP tool schemas are registered but not injected until needed. A database tool, a browser tool, and a search tool configured — none of them add tokens while you're editing source code.

---

## The Tradeoff

This approach isn't free.

**Trigger matching is naive.** "Make this more idiomatic" won't fire the Rust skill even in a Rust project, because the message doesn't contain "rust" or "cargo". Semantic matching would be smarter. It's a known gap.

**Skills need maintenance.** A bad skill is worse than no skill — it injects wrong conventions directly into every matching turn. The quality of output is now tied to the quality of the skill files.

**The baseline is still 1,239 tokens.** For simple questions, even the base prompt is overhead. The casual path catches the obvious cases, but the boundary is a heuristic.

---

## What the Architecture Looks Like

```
─────────────────────── Gemini CLI ────────────────────────
"Write a Spring Boot service"    "Build a React component"
         │                                │
         ▼                                ▼
  4,096 tokens                     4,096 tokens
  renderPreamble                   renderPreamble
  renderCoreMandates               renderCoreMandates
  renderPrimaryWorkflows           renderPrimaryWorkflows
  renderOperationalGuidelines      renderOperationalGuidelines
  renderSubAgents                  renderSubAgents
  renderGitRepo                    renderGitRepo
  renderSandbox                    renderSandbox
  ...                              ...
  Java content: 0 tokens           React content: 0 tokens
         │                                │
         └──────────── identical ─────────┘

──────────────────────── OpenCode ─────────────────────────
"Write a Spring Boot service"    "Build a React component"
         │                                │
         ▼                                ▼
  2,003 tokens                     2,003 tokens
  baseAnthropicCoderPrompt         baseAnthropicCoderPrompt
  (same constant)                  (same constant)
  Java content: 0 tokens           React content: 0 tokens
         │                                │
         └──────────── identical ─────────┘

────────────────────────── ZAP ────────────────────────────
"Write a Spring Boot service"    "Build a React component"
         │                                │
         ▼                                ▼
  Base: ~1,239 tokens            Base: ~1,239 tokens
  + java.md: ~650 tokens         + react.md: ~422 tokens
  ─────────────────              ─────────────────
  Total: ~1,889 tokens           Total: ~1,661 tokens

  LLM knows:                     LLM knows:
  ✓ Optional<T>, not nulls        ✓ Functional components only
  ✓ Constructor injection         ✓ React Query for data fetch
  ✓ JUnit 5 + AssertJ             ✓ Lift state, not prop drill
  ✓ Java 17+ record types         ✓ No class components
  ✗ No React rules                ✗ No Java rules
         │                                │
         └─────────── different ──────────┘
```

---

## The Code

ZAP is open source, written in Rust.

**GitHub:** https://github.com/zap-coding-agent/zap-coding-agent

The files described in this post:

- [`src/context_manager.rs`](https://github.com/zap-coding-agent/zap-coding-agent/blob/main/src/context_manager.rs) — prompt composition, casual path
- [`src/skill_manager.rs`](https://github.com/zap-coding-agent/zap-coding-agent/blob/main/src/skill_manager.rs) — skill discovery, trigger matching, three-tier priority
- [`src/session/mod.rs`](https://github.com/zap-coding-agent/zap-coding-agent/blob/main/src/session/mod.rs) — per-turn path selection, history window, pruning
- [`skills/`](https://github.com/zap-coding-agent/zap-coding-agent/tree/main/skills) — the 23 bundled skill files (readable, forkable, replaceable)

---

*This is part 2 of the ZAP series. Part 1: [overview and motivation](./medium.md). Part 3: code indexing with Tree-sitter.*

*Measurements gathered from Gemini CLI and OpenCode public GitHub repos on 2026-05-23. Raw data and methodology in [`content/evidence/system-prompt-comparison.md`](../evidence/system-prompt-comparison.md).*
