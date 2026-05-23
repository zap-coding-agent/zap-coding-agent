# LinkedIn — Series Overview

---

**Introducing ZAP — The Skill-First AI Coding Agent. No system prompt bloat. Real code intelligence. Built in Rust.**

---

Every AI coding agent today ships with a wall of system prompt instructions stuffed into every single request. It's wasteful, it dilutes context quality, and the LLM ends up reasoning through noise.

ZAP takes a different approach.

Here's what makes it different:

**1. Skill-first, not prompt-first**
Instead of one giant system prompt, ZAP injects only the skills relevant to your current task — progressively, as needed. Less token waste. More importantly: cleaner, higher-quality context for the LLM to reason over.

**2. Code indexing with AST (powered by Tree-sitter)**
ZAP doesn't guess at your codebase structure. It parses it. Tree-sitter gives ZAP a real understanding of symbols, definitions, and relationships — so the agent works with facts, not approximations.

**3. Lazy MCP loading**
MCP tools are loaded on demand, not upfront. Your context window stays lean until the agent actually needs a capability.

**4. Project initialization — make your codebase AI-ready**
One command to analyze your project, build its index, and configure ZAP for your stack. First-run experience matters, and most agents skip it entirely.

**5. CLI + TUI — your terminal, your choice**
Run ZAP as a scriptable CLI or drop into the full TUI for an interactive session. No Electron. No browser tab. No cloud you didn't choose.

This is the first post in a series breaking down how ZAP works under the hood — the architecture, the tradeoffs, and the things that turned out to be harder than expected.

More to come. Follow along.

---

*Next: how the skill injection system works and why system prompt bloat is a real problem.*

#Rust #AIAgents #DeveloperTools #CodingAgent #OpenSource
