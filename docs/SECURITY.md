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

## Reporting security issues

If you discover a security issue in zap's sandboxing or tool guards, please
open a GitHub issue with the `security` label.
