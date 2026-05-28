---
category: practice
name: demos
trigger: ["demo", "demos", "vhs", ".tape", "gif", "recording", "setup.sh", "run.sh", "scenario", "code_indexing", "index-only", "demo folder"]
tokens: ~600
---

## Creating zap demos

Demos live in `demos/<capability-name>/` and are self-contained — they clone a real public repo, index it with zap, and run SDK scenarios to show the feature in action.

### Folder structure

```
demos/<name>/
  setup.sh          # clone repo + zap --index-only
  run.sh            # run all scenarios, format output
  scenarios/
    01_<name>.sh    # printf JSON | zap --sdk --auto 2>/dev/null
    02_<name>.sh
  demo.tape         # vhs tape — hero GIF (scenario 1 only, ~30s)
  demo_full.tape    # vhs tape — all scenarios
  README.md         # what/why table, quick start, scenario table
```

### Headless indexing

Use `zap --index-only` in `setup.sh` to build the AST index without starting a session:

```bash
cd "$REPO_DIR"
zap --index-only    # writes .zap/code.db — same as /index inside a session
```

Do NOT send `/index` via `zap --sdk` — SDK mode passes text to the LLM, it never reaches the slash-command handler.

### SDK scenario scripts

```bash
printf '{"type":"user","text":"Where is the Flask class defined?"}\n' \
  | zap --sdk --auto 2>/dev/null
```

`--auto` approves all tool calls. `2>/dev/null` hides spinner/progress. stdout is newline-delimited JSON.

### vhs tape authoring

**PATH fix — always required.** vhs spawns a non-login shell; `~/.local/bin` is not on PATH. Add this inside the `Hide` block:

```
Hide
Type `export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"`
Enter
Sleep 200ms
Type "cd myrepo"
Enter
Ctrl+L
Show
```

**Dimensions are pixels, not columns.** Use `Set Width 1400` / `Set Height 700` — values under 120 error out.

**Theme name has no space.** Use `Set Theme "TokyoNight"`, not `"Tokyo Night"`.

**Do not use `Env` directives.** `Env VHS_NO_SANDBOX "1"` conflicts with `Set` directives and silently breaks them. Instead prefix the vhs command:

```bash
VHS_NO_SANDBOX=1 vhs demo.tape
```

**Set directives must come before any actions.** Order: `Output` → `Set *` → `Hide`/`Show`/`Type`/`Sleep`.

**Sleep budgets.** Allow ~30s per scenario for Deepseek (API latency). `Sleep 30s` for scenario 1, `Sleep 60s` for longer traces.

**Recording a GIF:**

```bash
cd demos/code_indexing
VHS_NO_SANDBOX=1 vhs demo.tape        # → demo.gif
VHS_NO_SANDBOX=1 vhs demo_full.tape   # → demo_full.gif
```

### README embed

```markdown
<!-- generate with: cd demos/code_indexing && VHS_NO_SANDBOX=1 vhs demo.tape -->
![code indexing demo](demos/code_indexing/demo.gif)
```

### .gitignore

Ignore cloned repos, not the generated GIFs (GIFs are the sharable artifact):

```
flask/
express/
requests/
```

### INDEX hit vs miss

Scenarios should show `INDEX hit` in tool call output, not `INDEX miss · grep fallback`. If you see misses, run `zap --index-only` again in the repo directory to rebuild the DB.
