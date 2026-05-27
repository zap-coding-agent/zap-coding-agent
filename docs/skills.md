# Skills — Context That Earns Its Place

Most AI coding agents front-load a massive system prompt every request — language conventions, architecture notes, team rules, API patterns, all of it, whether it's relevant or not. zap replaces that wall with a **skill system**: markdown files that are injected surgically, only when your message triggers them.

## Two kinds of skills

| Kind | When injected | Example |
|---|---|---|
| **Always-on** | Every turn, baked into the base system prompt | `karpathy-guidelines` — Andrej Karpathy's 4 coding principles |
| **Triggered** | Only when your message matches keywords | `rust` fires on "cargo", "fn ", "trait "; `git` fires on "commit", "push" |

## Built-in skills

| Skill | Type | Triggers on |
|---|---|---|
| `karpathy-guidelines` | always-on | every turn |
| `rust` | triggered | rust, cargo, crate, async fn, clippy… |
| `python` | triggered | python, pip, pytest, dataclass… |
| `typescript` | triggered | typescript, tsx, interface, npm… |
| `react` | triggered | react, component, jsx, hook, useState… |
| `go` | triggered | go, goroutine, chan, go.mod… |
| `git` | triggered | commit, branch, merge, pull request… |
| `code-review` | triggered | review, pr review, lgtm, critique… |
| `debugging` | triggered | debug, error, crash, panic, stacktrace… |
| `security` | triggered | auth, password, token, jwt, xss, sql injection… |

Stack auto-detection fires the right language skill on startup — a Rust project with `Cargo.toml` gets the `rust` skill loaded automatically.

## Token impact

A Rust project with a custom `api-conventions` skill loaded:

| You type | Skills injected | Base + skills |
|---|---|---|
| `"refactor this async fn to use channels"` | karpathy + rust | ~2.4k tokens |
| `"commit these changes"` | karpathy + git | ~2.0k tokens |
| `"add a new REST endpoint"` | karpathy + api-conventions | ~2.2k tokens |
| `"explain what this function does"` | karpathy only | ~1.8k tokens |

> **Honest baseline:** the always-on karpathy-guidelines skill and the base system prompt together run ~1.8k tokens — much leaner than Claude Code (~10k) or Gemini CLI (~8k), but not the "200 token" figure you might see in older docs.

## Writing skills

**Triggered skill** — injected only when keywords match:

```markdown
---
name: api-conventions
description: REST endpoint conventions for this project.
trigger: ["endpoint", "route", "handler", "REST"]
tokens: ~400
---
All endpoints must validate input with ValidateRequest(), return structured
errors as {"error": "...", "code": N}, and use snake_case JSON keys.
```

**Always-on skill** — no `trigger:` field, injected every session:

```markdown
---
name: our-principles
description: Team engineering principles.
---
We ship small, reversible changes. Every PR needs a test. No console.log in prod.
```

## Where to put skills

| Path | Scope | Priority |
|---|---|---|
| `.zap/skills/` | project — check into git, team-shared | highest |
| `~/.zap/skills/` | personal — all projects | middle |
| binary | built-in defaults | lowest |

On first launch zap writes all built-in skills to `~/.zap/skills/` automatically — open any file there to read or edit it. Same-name files you create override the built-in version on the next run.

## Commands

```
/skill list                      list all skills (grouped: always-on / triggered)
/skill show <name>               preview content, description, license
/skill log                       show which skills fired (or why they didn't) per turn this session
/skill scope                     show which domain skills are active this session
/skill scope add <name>          add a skill to the active scope
/skill scope remove <name>       remove a skill from the active scope
/skill scope reset               restore default scope
/skill export <name>             re-export a built-in to ~/.zap/skills/
/skill export --all              re-export every built-in skill
/skill create <name>             scaffold a new skill in .zap/skills/
/skill create <name> --global    scaffold in ~/.zap/skills/
/skill capture <name>            extract rules from this session into a skill file
/skill capture <name> --global   save captured skill globally
```

## Skill trace

`/skill log` lets you audit which skill fired (or didn't) for every turn this session. If a skill you expected to fire didn't, the log shows "no match" or "casual" with the turn preview so you can tune the trigger keywords:

```
  turn #3  "refactor the async fn to use channels"    → rust, karpathy-guidelines
  turn #4  "commit these changes"                     → git, karpathy-guidelines
  turn #5  "hey thanks"                               → (casual)
  turn #6  "add an endpoint for POST /users"          → (no match)  ← missing api-conventions skill?
```

## Multi-tool skill sources — Kiro, Claude Code, and custom dirs

If your project already has skills written for Amazon Kiro (`.kiro/skills/`) or Claude Code (`.claude/skills/`), you can pull them into zap without copying files. Add `skill_paths` to `~/.agent.toml`:

```toml
# ~/.agent.toml
skill_paths = [
    ".kiro/skills",          # Amazon Kiro skills (per-project)
    ".claude/skills",        # Claude Code skills (per-project)
    "~/shared-skills",       # your own cross-project library
]
```

zap loads every `.md` file it finds in those directories and merges them into the flat skill registry. Skills from `skill_paths` override global (`~/.zap/skills/`) but lose to project-local (`.zap/skills/`).

**Full precedence** (lowest → highest, later wins on name collision):

| Source | Location | Glyph in `/skill list` |
|---|---|---|
| Built-in | compiled into binary | `◆` |
| Global | `~/.zap/skills/` | `●` |
| External | `skill_paths` entries, left → right | `◉` |
| Project | `.zap/skills/` | `▶` |

`/skill list` shows the source tag next to every skill:

```
  ◆ rust             [built-in]   always-on
  ● karpathy-guidelines [global]  always-on
  ◉ api-design       [kiro/skills]  rest, endpoint, api
  ◉ code-review      [claude/skills]  code review, pr review
  ▶ team-principles  [project]    always-on
```

## Always-on context from other tools — Kiro steering, Claude context

Steering documents (`.kiro/steering/`) and Claude project context files aren't skills — they have no trigger and no frontmatter. Use `context_paths` to load them as always-on system context:

```toml
# ~/.agent.toml
context_paths = [
    ".kiro/steering",     # Kiro steering docs — loaded every session
    ".claude/context",    # Claude context docs
]
```

All `.md` files found in `context_paths` directories are appended to the system prompt every turn, after the main ZAP.md/CLAUDE.md. Frontmatter (`---` blocks) is stripped automatically. Files are sorted by filename within each directory.

> **Tip:** `skill_paths` and `context_paths` are complementary. Use `skill_paths` for keyword-triggered guidance and `context_paths` for always-on context that applies regardless of what you're asking.

## Skill compatibility

Skills follow the [SKILL.md standard](https://github.com/multica-ai/andrej-karpathy-skills) — compatible with Claude Code, Cursor, and other agents. Skills you write for zap work in other agents today.
