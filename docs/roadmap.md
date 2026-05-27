# Roadmap & Contributing

## Roadmap — Skill Ecosystem

zap's bet is on **skills as a platform**, not on being a better terminal agent. The goal: turn team knowledge into code, make it shareable, composable, and cross-compatible with other agents.

| Feature | Status | What it enables |
|---|---|---|
| `/skill install github:user/repo/path` | planned | One-command community skill install |
| Skill extends / composition (`extends: [rust, code-review]`) | planned | Composable skill layers |
| Semantic skill routing (embedding similarity instead of keyword match) | planned | Intent-based matching, no keyword guessing |
| Public skill directory (`zap.sh/skills`) | planned | Discoverable ecosystem |
| Stack auto-detection expansion (Ruby, Swift, Kotlin, C++) | planned | Zero-config for more users |
| Cross-agent compatibility testing | planned | Write once, use anywhere |

The skill format is already compatible with Claude Code (`CLAUDE.md`-style) and the [multica-ai SKILL.md standard](https://github.com/multica-ai/andrej-karpathy-skills). Skills you write for zap work in other agents today.

---

## Contributing

Contributions are welcome — bug fixes, new providers, language support, skill improvements, or anything that makes zap more useful.

### Reporting bugs

Open an issue at [github.com/sanjeev23oct/zap/issues](https://github.com/sanjeev23oct/zap/issues). Include your OS, model/provider, the command you ran, and what you expected vs what happened. Attach the relevant lines from `agent_audit.jsonl` if the problem is tool-related.

### Feature requests

Open an issue with the `enhancement` label. Describe the use case, not just the feature — it helps prioritise.

### Pull requests

1. Fork the repo and create a branch from `main`
2. Keep changes focused — one PR per fix or feature
3. Run `cargo check` and `cargo clippy` before submitting — zero warnings expected
4. Update the relevant doc in `docs/` if you're adding a visible feature

### Adding a built-in skill

Built-in skills live in `src/default_skills/`. Each is a markdown file with YAML frontmatter (name, trigger keywords, token estimate). If you have good conventions for a language or framework not yet covered, a skill PR is one of the easiest contributions to make.

### Adding a provider

Provider switching lives in `src/session.rs` (`cmd_provider`). All providers speak the OpenAI wire format — adding one is usually just a new entry in the picker with a `base_url` and default model.
