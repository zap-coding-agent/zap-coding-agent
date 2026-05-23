# zap e2e tests

End-to-end tests that run zap as a black box and assert on observable behaviour.

## Quick start

```bash
# Build and install first
cargo build --release && cp target/release/zap ~/.local/bin/zap

# Run everything
./tests/e2e/run_all.sh

# Run one suite
./tests/e2e/run_all.sh test_basic

# Use a different binary
ZAP_BIN=./target/release/zap ./tests/e2e/run_all.sh
```

## Test suites

| File | What it covers |
|---|---|
| `test_basic.sh` | Single-shot goal, plain answer, no panic |
| `test_tools.sh` | `list_directory`, `read_file`, `shell` tool use |
| `test_index.sh` | `/index` slash command, tree-sitter log lines, `.zap/code.db` |
| `test_init.sh` | `/init` flow, `project.json`, `ZAP.md`, no nudge on 2nd run |
| `test_session.sh` | Session end writes `context.md` and `session_log.md` |
| `test_tui.sh` | TUI starts with a PTY, renders banner, exits cleanly |
| `test_regression.sh` | Specific bugs: char-boundary panic, `/sessions` crash |

## TUI tests

`test_tui.sh` requires the `script` utility (standard on macOS/Linux) to allocate a
PTY, because ratatui takes over the terminal in raw mode and does not respond to
stdin pipes.  The test sends `/exit` after 3 seconds and checks the captured
scrollback.

Full interactive TUI actions (multi-turn conversation, permission dialogs, file
browser) cannot be automated without a headless terminal driver like `expect` or
`tmux` scripting.  Those scenarios should be tested manually or via a future
`expect`-based suite.

## Environment variables

| Variable | Default | Effect |
|---|---|---|
| `ZAP_BIN` | `~/.local/bin/zap` | Path to zap binary |
| `ZAP_TEST_TIMEOUT` | `60` | Per-test timeout in seconds |
