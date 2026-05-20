---
category: domain
name: bash
trigger: ["bash", "shell script", "#!/bin/bash", "#!/bin/sh", "shellcheck", ".sh", "posix shell", "zsh script", "sh script", "awk ", "sed ", "grep ", "cron", "makefile"]
tokens: ~550
---

## Bash / shell scripting conventions

**Source:** Google Shell Style Guide, ShellCheck rules, POSIX specification.

**Safety header — always start scripts with:**
```bash
#!/usr/bin/env bash
set -euo pipefail
IFS=$'\n\t'
```
`-e`: exit on error. `-u`: error on unset variable. `-o pipefail`: pipeline fails if any stage fails. `IFS`: prevents word-splitting surprises.

**Variables:**
- Quote every variable expansion: `"$var"` not `$var` (prevents word splitting on spaces/globs).
- Use `${var}` for unambiguous expansion inside strings.
- Prefer `local` for function variables — globals leak.
- Use UPPER_CASE for environment/exported variables, lower_case for locals.
- Default values: `${var:-default}`. Required: `${var:?Variable must be set}`.

**Functions:** Define before calling. Use `local` for all variables. Return status codes (0=success), not echoed strings — callers capture with `$()`.

**Conditionals:**
- Use `[[ ]]` (bash) not `[ ]` (POSIX) — safer string comparison, regex support.
- Quote strings inside `[[ ]]` for safety: `[[ "$var" == "value" ]]`.
- Never parse `ls` output — use glob patterns or `find` instead.
- Check command existence: `command -v git >/dev/null 2>&1 || { echo "git required"; exit 1; }`.

**Error handling:** Print errors to stderr: `echo "Error: msg" >&2`. Trap for cleanup: `trap 'cleanup' EXIT`. Use meaningful exit codes.

**Portability:** For scripts that must run on older systems, avoid bash-specific features and stick to POSIX sh. If using bash 4+ features, document the minimum version.

**Subprocess:** Prefer `$(cmd)` over backticks. Avoid parsing output of commands that can contain newlines without `IFS` control.

**Testing:** Use `bats-core` (Bash Automated Testing System) for unit-testable scripts. Run all scripts through `shellcheck` in CI.

**Formatting:** 2-space indent. Max line 80 chars. One statement per line.
