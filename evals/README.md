# evals — eval harness for zap

A repeatable way to ask "is the agent getting better or worse" across changes.
Runs a catalogue of small, hermetic tasks against the compiled `zap` binary in
SDK mode and reports pass rate, token usage, estimated cost, and per-task
status.

## Quickstart

```bash
# 1. Build the agent.
cargo build --release                  # produces target/release/zap

# 2. List the task catalogue (no LLM calls; works without an API key).
cargo run --release --bin evals -- --list

# 3. Run all tasks.  zap reads ~/.agent.toml for its provider and key —
#    you don't need to pass them to the harness.  Pass --model purely so
#    the cost-estimate column uses the right pricing.
cargo run --release --bin evals -- --model claude-opus-4-8

# 4. Run a subset.
cargo run --release --bin evals -- --only 01_create_file,15_append_only
```

A results JSON is written to `evals/results/<timestamp>.json` and a summary
table is printed to stdout. Exit code is 0 if every non-skipped task passes,
1 otherwise.

## Command-line flags

| Flag | Default | Purpose |
|---|---|---|
| `--zap-bin <path>` | `target/release/zap` | Path to the compiled `zap` binary |
| `--tasks-dir <path>` | `evals/tasks` | Directory of `*.json` task files |
| `--results-dir <path>` | `evals/results` | Where to write the timestamped results JSON |
| `--model <name>` | unset | Recorded in results + drives the cost estimate (does NOT override `~/.agent.toml`) |
| `--list` | off | Print the catalogue and skip checks; no LLM calls |
| `--only <id,id,...>` | unset | Run only the named task ids |
| `--no-color` | off | Plain stdout, no ANSI escapes |

## Task schema

One `.json` file per task in `evals/tasks/`:

```json
{
  "id": "01_create_file",
  "category": "edit",
  "description": "Create a new file with specified contents.",
  "prompt": "Create a file called hello.txt with the single line 'hello world' (no trailing whitespace).",
  "setup": "",
  "check": "test -f hello.txt && grep -qx 'hello world' hello.txt",
  "timeout_secs": 120,
  "max_turns": 6,
  "requires": []
}
```

Fields:

- **`id`** — unique, used by `--only` and as the table row label.
- **`category`** — free-form tag (`edit`, `search`, `bugfix`, `refactor`,
  `shell`, `refusal`, etc.). Used for the by-category breakdown.
- **`description`** — short human label (shown in `--list`).
- **`prompt`** — the message sent to the agent as one user turn.
- **`setup`** — a `bash -eu -o pipefail` script that runs inside the temp
  workspace *before* the agent starts. Use it to scaffold the files the
  agent will edit or read. Empty string = no setup.
- **`check`** — a `bash -eu -o pipefail` script that runs *after* the agent
  finishes. Exit 0 = pass, anything else = fail. Stay hermetic (no network,
  no project-level deps the runner doesn't have).
- **`timeout_secs`** — wall-clock cap for the agent run (not the check).
- **`max_turns`** — currently advisory; the agent enforces its own
  `MAX_TURNS=50` internally.
- **`requires`** — list of binaries that must be on `PATH` for the task to
  run (e.g. `["python3"]`). If any is missing, the task is marked
  `SKIP` rather than `FAIL`.

Each task runs in a **fresh `tempfile::tempdir()`**, so tasks never see
each other's state and never touch the host repo.

## Output

Stdout shows a summary like:

```
┌── Eval Results ──────────────────────────────────────────────
│
│  Pass rate: 12/14 tasks passed   1 skipped
│  Cost: ~0.0234 USD   (18432 in, 1203 out tokens)
│  Time: 142.3s total wall time
│
│  By category:
│    edit               7/8
│    refusal            1/1
│    search             3/3
│    shell              1/1
│    refactor           1/2
│
│  Per task:
│    id                           pass turns     tokens  wall   detail
│    01_create_file                PASS     2       1432   8.4s
│    …
└──────────────────────────────────────────────────────────────
```

The full per-task record (including the error string on failures) is in the
results JSON.

## Adding a task

1. Create `evals/tasks/<NN>_<short_name>.json` following the schema above.
2. Keep `setup` and `check` deterministic — no `$RANDOM`, no network, no
   global state.
3. Use `requires` when the check depends on a toolchain (`python3`, `node`,
   `cargo`, etc.) so the harness skips gracefully on machines without it.
4. Run `cargo run --release --bin evals -- --list` to confirm it's picked
   up, then `--only <your_id>` to verify it works end-to-end.

## Notes

- The harness sets the spawned `zap`'s `cwd` to the temp workspace, so file
  paths in the prompt and check are relative to that dir.
- Cost estimates are rough — the table is `(input_tokens, output_tokens) ×
  per-million rate` for a handful of known model families. Unknown models
  show 0 cost; pass `--model` to override which table is used.
- The results JSON is the durable record; the printed table is a courtesy.
  Diff two results files to spot regressions.
- For task development, you can pass `--zap-bin target/debug/zap` to skip
  the release build during iteration.
