# CI / Headless Mode & SDK

## Headless / CI mode

zap runs fully non-interactively. Add `--auto` (or `AGENT_PERMISSION_MODE=auto`) to skip all permission prompts:

```bash
# single-shot — clean for scripts
zap --auto --goal "review staged changes and write a summary to REVIEW.md"

# environment variable alternative
AGENT_PERMISSION_MODE=auto zap --goal "run cargo test and fix any failures"
```

### GitLab CI example

```yaml
# .gitlab-ci.yml
ai-review:
  image: ubuntu:24.04
  variables:
    ANTHROPIC_API_KEY: $ANTHROPIC_API_KEY   # set in CI/CD → Variables
  before_script:
    - curl -L https://github.com/sanjeev23oct/zap/releases/download/latest/zap-linux-x86_64
        -o /usr/local/bin/zap && chmod +x /usr/local/bin/zap
  script:
    - zap --auto --goal "review the diff since origin/main, identify bugs or missing tests,
        and write a report to ai-review.md"
  artifacts:
    paths: [ai-review.md]
    expire_in: 1 week
```

### GitHub Actions example

```yaml
- name: AI code review
  env:
    ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
  run: |
    zap --auto --goal "read the changed files, add docstrings where missing, and commit"
```

---

## SDK / Remote Control Mode

`--sdk` turns zap into a **JSON-lines server** — stdin carries prompts, stdout carries responses. It keeps session state across turns, so context accumulates.

```bash
zap --sdk          # stdin → stdout, --auto implied, no banner
```

### Protocol

**stdin** (one JSON object per line):
```json
{"type":"user","text":"refactor the auth module to use JWT"}
{"type":"user","text":"now write tests for the new auth module"}
{"type":"quit"}
```

**stdout** (one JSON object per line):
```json
{"type":"assistant","text":"I've refactored the auth module...","turn":1,"ctx_pct":12,"usage":{"input_tokens":1842,"output_tokens":487}}
{"type":"assistant","text":"I've written tests for...","turn":2,"ctx_pct":24,"usage":{"input_tokens":3210,"output_tokens":612}}
```

All terminal noise (tool call boxes, spinners) goes to **stderr** — stdout is clean JSON for machine consumption.

### Remote control over SSH

```bash
ssh user@dev-server 'ANTHROPIC_API_KEY=sk-ant-... zap --sdk' << 'PROMPTS'
{"type":"user","text":"run cargo test and fix any failures"}
{"type":"quit"}
PROMPTS
```

### Python script example

```python
import subprocess, json, os

proc = subprocess.Popen(
    ["zap", "--sdk"],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    env={**os.environ, "ANTHROPIC_API_KEY": "sk-ant-..."},
)

def ask(prompt: str) -> dict:
    proc.stdin.write(json.dumps({"type": "user", "text": prompt}).encode() + b"\n")
    proc.stdin.flush()
    return json.loads(proc.stdout.readline())

reply = ask("add input validation to src/api.rs")
print(reply["text"])

proc.stdin.write(b'{"type":"quit"}\n')
proc.stdin.flush()
proc.wait()
```
