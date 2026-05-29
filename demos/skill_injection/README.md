# Demo: Skill Injection

**Capability:** Context-aware skill injection — right knowledge, right turn  
**Repo:** [pallets/flask](https://github.com/pallets/flask) — Python web framework  
**Runtime:** ~30–45 s per scenario (Deepseek; mostly API latency)

---

## What this demo shows

zap ships with 24 built-in skills (language conventions, git, code review, security, etc.)
It does **not** dump them all into every request. Instead, it reads the user's message,
matches relevant skills by keyword, and injects only those — per turn.

### What to watch for

| Scenario | Trigger | What you see |
|----------|---------|--------------|
| `01_python_skill.sh` | "Flask route" + `pyproject.toml` in CWD | `↳ skills: python (~550t)` — Python conventions injected |
| `02_two_skills.sh` | "commit" + "Flask" | `↳ skills: python, git (~950t)` — two skills, one turn |
| `03_casual.sh` | "Nice, thanks!" | *(no `↳` line)* — casual path, minimal prompt |

The amber `↳ skills:` line is the signal. When it's absent on a casual message, that's the
point: zap sends ~200 tokens instead of ~2000, because the user wasn't asking for code help.

### With injection vs without

| | Always-on (competitors) | zap |
|--|------------------------|-----|
| Casual "thanks" | ~2 000 tokens (all rules always sent) | ~200 tokens (casual prompt) |
| Python question | ~2 000 tokens | ~750 tokens (base + python skill) |
| Python + git question | ~2 000 tokens | ~950 tokens (base + python + git) |

The math: a typical session of 20 turns with always-on context wastes 40k tokens before
the LLM reads a single line of your code.

---

## Quick start

```bash
# 1. Clone Flask and build the index
./setup.sh

# 2. Run all three scenarios (live output)
./run.sh

# 3. Record demo GIFs (requires: brew install vhs)
VHS_NO_SANDBOX=1 vhs demo.tape        # → demo.gif       (scenario 1, ~30 s)
VHS_NO_SANDBOX=1 vhs demo_full.tape   # → demo_full.gif  (all 3, ~2.5 min)
```

## Scenarios

| Script | Prompt | Skills fired |
|--------|--------|-------------|
| `scenarios/01_python_skill.sh` | "How should I handle database errors in a Flask route?" | `python` |
| `scenarios/02_two_skills.sh` | "Write a git commit for adding request-id tracing middleware to Flask" | `python` + `git` |
| `scenarios/03_casual.sh` | "Nice, thanks!" | *(none — casual path)* |
