# ⚡ zap — Overview Demo

5 features in ~4 minutes. Live binary, real AI responses, nothing pre-recorded.

## Prerequisites

```bash
brew install vhs        # terminal recording tool
cargo build --release   # build zap binary
```

The demo runs from `demos/code_indexing/flask/` — the Flask source is already checked out
and the AST index is pre-built there.

## Generate the video

```bash
cd demos/overview
VHS_NO_SANDBOX=1 vhs overview.tape
# Output: overview.mp4
```

## What it covers

| # | Feature | What to watch for |
|---|---|---|
| 1 | Skill injection | `↳ skills: python` — skill fires on matching keywords |
| 2 | Code index | `find_definition("Flask")` → exact file:line in 1 call |
| 3 | Casual turns | No `↳ skills` line — greeting costs ~31 tokens |
| 4 | `/init` | Auto-generates project context files |
| 5 | Security | ask / auto / deny permission modes |
