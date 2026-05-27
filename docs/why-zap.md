# Why zap — The Prompt Bloat Problem

## The Problem Every AI Coding Agent Has

Open any popular AI coding agent and inspect the raw request it sends to the LLM. You'll find hundreds — sometimes thousands — of lines of system prompt sent on **every single turn**, regardless of what you're actually doing.

We measured this. Here's what Gemini CLI and OpenCode send when you ask them to write a Spring Boot service vs. a React component — two completely different languages, frameworks, and conventions:

| | Gemini CLI | OpenCode | zap |
|---|---|---|---|
| Spring Boot request | **4,096 tokens** | **2,003 tokens** | 1,889 tokens |
| React request | **4,096 tokens** | **2,003 tokens** | 1,661 tokens |
| Prompts identical? | ✅ Yes — same bytes | ✅ Yes — same bytes | ❌ No — different skill injected |
| Java conventions in prompt? | ❌ None | ❌ None | ✅ 650 tokens |
| React conventions in prompt? | ❌ None | ❌ None | ✅ 422 tokens |

**Gemini CLI sends the same 4,096-token prompt for both.** The word "java" does not appear anywhere in its 68,410-character prompt file. Neither does "react", "kotlin", or any other language. The LLM writing your Spring Boot service and the LLM writing your React component receive identical instructions. ([source](https://github.com/google-gemini/gemini-cli/blob/main/packages/core/src/prompts/snippets.ts))

**OpenCode uses a single static string constant** — `baseAnthropicCoderPrompt` — sent verbatim on every turn, every task type. Zero mentions of Java, TypeScript, Rust, Python, React, or any specific language. ([source](https://github.com/opencode-ai/opencode/blob/main/internal/llm/prompt/coder.go))

This isn't just waste. It's why these agents give inconsistent output — the model has no language-specific guidance, so it invents its own conventions turn by turn.

zap sends a **different prompt for different tasks** — the Java skill fires for Spring Boot, the React skill fires for components — and a greeting costs 12 tokens, not 2,000–4,000.

> Full methodology, raw token counts, and source links: [`content/evidence/system-prompt-comparison.md`](../content/evidence/system-prompt-comparison.md)
> Medium series: [Introducing ZAP — The Open-Source AI Coding Agent That Doesn't Bloat Your LLM Context](../content/overview/medium.md)

---

## Context Quality is Supreme

Every design decision in zap follows one principle: **every token in the LLM's context window must earn its place.** Context that doesn't improve output quality is waste — it dilutes attention, burns budget, and produces inconsistent results.

This principle drives everything:

| Mechanism | What it ensures |
|---|---|
| **Skill injection** | Only language- and task-specific guidance is sent, not a one-size-fits-all monolith |
| **AST code index** | The agent knows what exists *before* it decides what to create — no blind writes |
| **`/init` command** | Auto-generates project-specific context files so every session starts informed |
| **Context files** | `ZAP.md`, `.zap/understanding.md`, `.zap/context.md`, `.zap/session_log.md` — maintained automatically, loaded on demand |
| **Casual-turn detection** | Greetings cost ~31 tokens, not 2,000–4,000 |
| **Lazy MCP** | Server tool schemas stay out of context until the model explicitly connects them |
| **Token-cost display** | Every turn shows exactly what went into context — skills, message, system, and estimated $ |

### What gets updated and when

zap maintains four project context files, each updated at a specific time to keep the agent current without polluting the context window:

| File | Updated when | What it contains | Loaded when |
|---|---|---|---|
| `ZAP.md` | `/init` (manual, once) | Project overview, build commands, architecture, do-not-touch list | Every session (on-demand) |
| `.zap/understanding.md` | `/init` (manual, once) | Deep technical map: modules, data flows, patterns, constraints | Every session (on-demand) |
| `.zap/context.md` | End of every session (auto) | Last session: goal, files touched, what's next | Session start (on-demand) |
| `.zap/session_log.md` | Every session (auto) | History of all past sessions, indexed by date | On request (on-demand) |

These files are never pre-loaded into the context window — the model reads them *only when relevant*, using `read_file`. This means a session about fixing a typo doesn't pay the cost of loading the entire project architecture.

### The result

- **First session:** `/init` analyses your repo and bootstraps all project knowledge in ~30 seconds
- **Returning sessions:** The agent already knows what you were working on, which files changed, and what was left unfinished
- **Every turn:** Skills inject only what's relevant, the index tells the agent what exists, and casual messages skip the overhead entirely

### Token efficiency — smart context detection

Pure greetings and social messages use a minimal 31-token prompt with no tools, even mid-conversation. Everything else gets the full context injection:

| Message | After model asks a question? | Result |
|---|---|---|
| "hi", "hello", "hey" | yes | ~31 tokens (casual) |
| "thanks", "thank you", "ty" | yes | ~31 tokens (casual) |
| "good morning", "how are you" | yes | ~31 tokens (casual) |
| "yes" | yes | full context |
| "go ahead" | yes | full context |
| "ok", "cool", "sounds good" | yes | full context |
| any technical question | — | full context |

After the model asks a clarifying question, short replies like "yes", "ok", "go ahead" are treated as answers and receive the full context. Pure social messages always stay casual.
