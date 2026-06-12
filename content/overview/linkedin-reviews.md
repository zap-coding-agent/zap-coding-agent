# LinkedIn — Independent Reviews Post

---

zap continues to evolve.

I had its engineering and security independently graded by the two most capable models on the market — Claude **Opus 4.8** and Claude **Fable 5** (the latter under the name Mythos). Source-only reads. No demos, no marketing decks. Both scored what was actually in `src/**`.

**Engineering — 8.5 / 10 (Opus 4.8)**

The verdict: clean architecture, the right abstractions, and three capabilities the other major agents simply do not ship —

- **A persistent code graph** — tree-sitter + SQLite, with PageRank-ranked files. The model answers "who calls X" or "what breaks if I rename Y" in one query, not ten greps. No other major agent maintains a queryable graph.
- **Per-task skill injection** — only the skills your message triggers are loaded, ranked and capped to a token budget. A Rust task gets Rust conventions; a greeting costs ~31 tokens. Others ship a static 2k-token wall on every turn.
- **Token-smart MCP** — servers stay pending at startup; tool schemas load on demand, not eagerly.

Architecture and code-understanding both scored 9/10. The remaining gap is operational maturity — eval baselines and real-world mileage — not coding ability.

**Security — 9.0 / 10 (Mythos · Fable 5)**

Started at 6.5. Six trust-boundary findings, each traced to a specific `file:line`. Two hardening passes later (v0.15.11, v0.15.12) the posture is 9.0 —

- `/remote` is back behind a per-session token gate
- File writes are jailed to the workspace; the path guard is symlink-safe
- Egress is scanned for credentials at every entry point, not just tool results
- A `cargo-audit` CI gate watches the dependency tree
- Project-local hooks and MCP servers require explicit directory trust

The remaining point is the single-user trust model — a deliberate design for a per-developer tool, not an open hole.

**Why this matters**

zap is the only AI coding agent I know of that shipped with two independent reviews — done by the most capable AI models out there — and the commits that closed every finding the reviewers named. Evidence, not adjectives.

If you're evaluating coding agents for your team — or doing technical due diligence — both reports are public:

→ Engineering review (8.5/10): https://zap.justpush.cloud/review.html
→ Security review (9.0/10): https://zap.justpush.cloud/security.html
→ Repo: https://github.com/zap-coding-agent/zap-coding-agent

Single Rust binary. Works with any provider including local / air-gapped. MIT licensed. Zero telemetry.

#Rust #AIAgents #DeveloperTools #CodingAgent #OpenSource #AISecurity
