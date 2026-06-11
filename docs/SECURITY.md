# Security Model

## What zap runs

zap is a coding agent that executes shell commands and edits files under the
user's identity.  By default it runs with the same privileges as the user
invoking it — full access to the filesystem, network, and processes that the
user has.

## What the shell tool can do

The `shell` tool lets the LLM run arbitrary commands via `sh -c`.  There is a
**confused-model guardrail** (a substring denylist) that blocks obviously
destructive patterns (`rm -rf /`, `mkfs`, `:(){ :|:& };:` etc.).  This
guardrail is **not a security boundary** — it is trivially bypassed with
encoding, variable indirection, or path tricks.  Its purpose is to catch
low-effort LLM mistakes, not to stop a determined adversary or a jailbroken
model.

## Sandbox modes

Set `sandbox` in `~/.agent.toml` (or `AGENT_SANDBOX` env var) to one of:

| Mode | Behaviour | Security boundary? |
|------|-----------|-------------------|
| `off` (default) | No restriction — commands run as the user | No |
| `workdir` | `current_dir` set to project root; commands that reference absolute paths outside the project tree will fail (file-not-found / permission-denied) | Weak — protects files outside the project, but the project itself is fully writable |
| `container` | Each command runs in a disposable Docker/Podman container (`--network none`, `--tmpfs /tmp`, project mounted read-only) | Strong — the host filesystem is read-only and the network is off. Falls back with an error if neither Docker nor Podman is on `PATH` |

### Workdir mode details

- Sets `current_dir(std::env::current_dir())` on the child process.
- Does **not** parse the command to reject paths — if the LLM references
  `/etc/passwd`, the command runs but the OS-level path resolution will fail
  because the working directory is the project root.  This is an *actual
  boundary* but not a hardened one.
- Environment variables are inherited from the parent process (as in all
  modes).

### Container mode details

- Requires **Docker** or **Podman** on `PATH`.  If neither is found, the
  command fails with an error.
- The container image is `alpine:latest`.  No extra packages are installed.
- Execution wrapper:
  ```
  docker run --rm --network none \
    -v '/project/path':'/project/path':ro \
    --tmpfs /tmp:exec \
    -w '/project/path' \
    alpine:latest sh -c '<escaped-command>'
  ```
- **What is isolated:** filesystem (project is read-only, host is invisible),
  network (none), process namespace.
- **What is NOT isolated:** CPU, memory, and disk I/O — a fork-bomb or
  memory-exhaustion command can still impact the host.  The container has
  write access to `/tmp`.
- The container is destroyed immediately after the command completes
  (`--rm`).

## Threat model

### What we protect against

1. **Confused-model footguns** — An LLM that is trying to help but generates
   a destructive command (`rm -rf` on the wrong directory, `git push --force`
   to the wrong branch).  The substring denylist catches the most common
   patterns.

2. **Filesystem escape** — In `workdir` mode, accidental writes outside the
   project root are blocked by the working-directory jail.  In `container`
   mode, the host filesystem is read-only.

3. **Network exfiltration** — In `container` mode, the network is disabled
   (`--network none`), preventing the LLM or any tool from phoning home.

### What we do NOT protect against

1. **Jailbroken / adversarial models** — If the model is actively trying to
   cause harm, the substring denylist is trivial to bypass.  Use `container`
   mode for meaningful isolation.

2. **Shell injection within the tool** — The `shell` tool intentionally
   passes commands to `sh -c`.  There is no sanitization of the command
   string beyond the denylist.  This is by design: the tool is meant to let
   the LLM run arbitrary commands.

3. **Resource exhaustion** — The container does not have CPU/memory limits.
   An LLM could still run `:(){ :|:& };:` or allocate large amounts of
   memory.

4. **Supply chain attacks** — If the LLM runs `pip install` or `npm install`,
   that code executes inside the container (or on the host in `off`/`workdir`
   mode).  There is no vetting of downloaded packages.

5. **File editing tools** — The `edit_file`, `write_file`, and `batch_edit`
   tools always write to the host filesystem and are **not affected by
   sandbox mode**.  They respect `guard_path` (which blocks writing outside
   the project root and to hidden/system directories), but this is also a
   guardrail, not a security boundary.  In `container` mode the shell is
   isolated but file edits are not — the LLM could use `write_file` to
   overwrite project files even in container mode.

6. **User-initiated actions** — If the user explicitly approves a dangerous
   command via the permission dialog, zap will execute it regardless of
   sandbox settings.

## Recommendations

| Risk profile | Recommended mode |
|--------------|-----------------|
| Exploring new codebases, reading docs | `off` |
| Editing your own project | `off` (file-tool guard_path protects outside project) |
| Running untrusted or public LLMs | `container` |
| Multi-tenant / CI environments | `container` |

## Project trust (hooks & MCP)

Hooks (`.zap/hooks.json`) and MCP servers (`.mcp.json`) execute code on your
machine.  zap loads them from two places:

- **Global** (`~/.zap/hooks.json`, `~/.zap/mcp.json`) — your own machine config;
  always loaded.
- **Project-local** (`.zap/hooks.json`, `.mcp.json` in the working directory) —
  ships inside the repo.  These run **only when the directory is trusted**, so
  cloning and opening an untrusted repository does not run its `SessionStart`
  hook or spawn its MCP servers.

A directory is trusted when any of these hold:

- env `ZAP_TRUST_PROJECT` is `1` / `true` / `yes`
- a `.zap/trusted` marker file exists in the project
- the project's canonical path is listed in `~/.zap/trusted_dirs`

When project-local config is skipped, zap prints a one-line notice telling you
how to opt in.  Only trust repositories you have reviewed.

## File-tool path guard

`read_file`, `write_file`, `edit_file`, and `batch_edit` run every path through
a guard that (1) resolves symlinks to their real on-disk target before checking
(so a link inside the project cannot reach a blocked location) and (2) rejects a
denylist of credential stores (`~/.ssh`, `~/.aws`, `~/.config/gcloud`,
`~/.config/gh`, `~/.npmrc`, SSH key files by name, shell history, `~/.agent.toml`,
`/etc/{passwd,shadow,sudoers}`, and more).

**Writes are additionally jailed.**  `write_file`, `edit_file`, and `batch_edit`
require the resolved target to live under the **project root, the system temp
dir, or a configured `allowed_paths` root** — a prompt-injected or confused
overwrite cannot escape the workspace.  Add extra write roots in `~/.agent.toml`:

```toml
allowed_paths = ["~/scratch", "/data/out"]
```

Reads stay intentionally broad (zap legitimately reads arbitrary project files,
temp files, and `/dev/null`) but remain symlink-safe and denylisted.  Use the
`shell` tool if write access to a path outside the jail is genuinely intentional.

## Outbound secret scanning

Before content is sent to a cloud model, zap scans for credentials at every entry
point: **tool results** are redacted, **injected project context** (`ZAP.md`,
`context_paths`) is redacted, and **the user's own message** is scanned and a
visible warning is shown (user-typed text is warned, not auto-redacted — it may be
intentional).  Detection combines ~40 known credential patterns with an
**entropy detector** for random tokens that match no known prefix, tuned to avoid
false positives on git SHAs and hashes.  Local / LAN endpoints are exempt — that
content never leaves the network.

## Remote control (`/remote`)

`/remote` tunnels the session to a public URL, gated by a **per-session access
token**.  The token is drawn from the OS CSPRNG and appended to the printed URL
(`?token=…`); both the page and the WebSocket upgrade reject any request without
it, so a leaked URL minus the token is inert.  `/remote` also **refuses to start
in `auto` permission mode**, where a leaked URL could otherwise drive the shell
unattended — switch to `ask` mode first.  Keep the printed URL secret; the token
is the only access control.

## Reporting security issues

If you discover a security issue in zap's sandboxing or tool guards, please
open a GitHub issue with the `security` label.
