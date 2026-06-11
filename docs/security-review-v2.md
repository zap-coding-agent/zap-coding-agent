# zap — Independent Security & Vulnerability Review (v2)

**Reviewer:** Mythos (independent security assessment)
**Date:** 2026-06-10
**Version reviewed:** 0.15.11 (`Cargo.toml`) — up from 0.15.10 in v1
**Supersedes:** `docs/security-review.md` (v1, posture 6.5/10)
**Method:** Re-read of the touched source after the v1 findings were remediated. Every status change below is traced to code I read and a build + test run I watched pass — not to commit messages.

---

## TL;DR

v1 found a strong foundation with the gaps concentrated at the **edges of the trust model** — the places where input arrives from somewhere other than the local terminal. The team turned around fixes for all six findings, and the two that actually moved the needle (the unauthenticated public tunnel, and code-execution-on-repo-open) are closed.

- **`/remote` is disabled** — the unauthenticated public tunnel can no longer be started. This removes the only HIGH finding.
- **Project-local hooks and MCP servers now require explicit trust** — cloning and opening an untrusted repo no longer runs its `SessionStart` hook or spawns its MCP servers.
- **The file-path guard resolves symlinks** before checking, closing the bypass, and the **credential denylist was substantially hardened**.
- **The secret pre-flight scanner gained coverage**, and the **session DB is now `0600`**.

Build is clean with zero warnings; the test count rose **168 → 171** (the 3 new tests cover the trust gate).

**Overall security posture: 8.0 / 10.** Up from 6.5. The remaining gap is honest residual risk that needs design work, not a quick patch: `/remote` is disabled rather than fixed-with-auth, and the file/egress guards are hardened denylists rather than allowlists. Both are documented below as open.

---

## What changed, finding by finding

| # | v1 Sev | Finding | v2 Status | Evidence |
|---|--------|---------|-----------|----------|
| 1 | HIGH | `/remote` public tunnel, no auth | **Mitigated — disabled** | `tui/commands/mod.rs:228` |
| 2 | MEDIUM | `read_file` not confined; thin denylist | **Mitigated — hardened denylist + symlink resolve** | `tools/file/mod.rs:39` |
| 3 | MEDIUM | Path guard symlink bypass | **Fixed** | `tools/file/mod.rs` `resolve_symlinks` |
| 4 | MEDIUM | Secret scanner gaps | **Improved** | `secret_scanner.rs` |
| 5 | MEDIUM | Project hooks/MCP run untrusted code | **Fixed — trust gate** | `trust.rs`, `hooks.rs`, `mcp.rs` |
| 6 | LOW | Session DB unencrypted / world-readable | **Fixed — `0600`** | `persistence.rs:18` |

---

### Finding 1 — `/remote` — **MITIGATED (disabled)** ✅

The `/remote` start path is now a no-op that returns a message explaining why (`tui/commands/mod.rs:228-249`). The server and tunnel can no longer be launched, so the unauthenticated public-ingress path is gone. `/remote stop` still works so anyone with an already-running server from an older build can tear it down after upgrading.

**Residual (open):** this is removal, not a real fix. The feature is valuable and should return behind a **per-session access token** appended to the printed URL and required on the `/ws` upgrade, plus a refusal to tunnel when `permission_mode == Auto`. Until that lands, remote driving is unavailable by design. Tracked as the top item for the next security cycle.

### Finding 2 — file-path confinement — **MITIGATED** ✅

`guard_path` now (a) **resolves symlinks** before any check (see Finding 3) and (b) matches against a **much wider credential denylist** (`tools/file/mod.rs:39-110`): SSH/GPG key files by name (`id_rsa`, `id_ed25519`, …), all major cloud credential stores (`~/.aws`, `~/.azure`, `~/.config/gcloud`, `~/.kube`, `~/.docker`, podman `auth.json`, `~/.oci`), VCS/registry tokens (`~/.config/gh`, `~/.git-credentials`, `~/.netrc`, `~/.npmrc`, `~/.pypirc`, `~/.cargo/credentials`, `~/.gem/credentials`, Terraform creds), DB credentials (`~/.pgpass`, `~/.my.cnf`), and shell history.

**Why not a hard jail:** v1 recommended confining file tools to the project root. In practice zap legitimately reads `/dev/null`, temp files, and user-referenced paths outside the repo — a hard allowlist breaks normal agent operation (and four existing tests confirm that reliance). The engineering-honest choice was a hardened denylist plus symlink resolution, which closes the credential-exfiltration path that actually motivated the finding without crippling the tool.

**Residual (open):** it remains a denylist, not an allowlist — a credential store not on the list is still readable. Acceptable for a single-user local coding agent; revisit if zap ever runs against untrusted prompts in a multi-tenant context.

### Finding 3 — symlink bypass — **FIXED** ✅

New `resolve_symlinks()` (`tools/file/mod.rs`) walks up to the nearest existing ancestor, canonicalizes it (resolving every symlink in the prefix), then re-appends the not-yet-existing tail. `guard_path` runs its denylist against this resolved path. A symlink inside the project that points at `~/.ssh/id_rsa` now resolves to its real target and is blocked. Works for both reads and the create-new-file write path.

### Finding 4 — secret pre-flight scanner — **IMPROVED** ✅

The pattern set roughly doubled (`secret_scanner.rs`): added Google `AIza` keys, Hugging Face `hf_`, more GitHub token prefixes (`ghu_`, `ghr_`), Slack (`xoxb-`/`xoxp-`/`xapp-` + webhook URLs), Azure connection strings and storage keys, npm `_authToken`, OAuth `client_secret`, `Authorization: Bearer` headers, and credentialed DB connection strings (`postgres://`, `mysql://`, `mongodb://`, `redis://`, …). Over-broad needles were deliberately rejected during this work (`sk-` matches "task-", `asia` matches "Malaysia", `npm_` matches `npm_package_*`) to avoid redacting legitimate tool output.

**Residual (open):** still a case-insensitive substring matcher that scans **tool results only** — secrets the user types into a prompt, or content injected via `context_paths`/skills, are not scanned. Marketing should describe this as "best-effort secret pre-flight," matching the honest tone of `SECURITY.md`. Broadening to outbound user messages is the next improvement.

### Finding 5 — project trust gate — **FIXED** ✅

New `src/trust.rs` centralizes a `project_trusted()` check. **Global config under `~/.zap/` always loads** (it's the user's own machine setup). **Project-local config now loads only when the directory is trusted:**

- `hooks.rs:load()` skips `.zap/hooks.json` in an untrusted directory and prints a one-line warning telling the user how to opt in.
- `mcp.rs:load_config()` skips a project `.mcp.json` the same way.

A directory is trusted when `ZAP_TRUST_PROJECT=1`, or a `.zap/trusted` marker file exists, or the canonical path is listed in `~/.zap/trusted_dirs`. Result: cloning a repo with a malicious `SessionStart` hook and opening it no longer executes that hook. Covered by 3 new unit tests.

### Finding 6 — session DB permissions — **FIXED** ✅

`persistence.rs::open()` now sets `0600` on `~/.zap/agent.db` at open time (unix), mirroring the `~/.agent.toml` treatment. Conversation history (which can contain secrets the model handled) is no longer readable by other users on a shared machine.

---

## Updated scorecard

| Dimension | v1 | v2 | Notes |
|---|---|---|---|
| Memory & language safety | 9 | 9 | Unchanged — safe Rust |
| Credential handling (at rest / in logs) | 8 | **9** | DB now `0600` |
| Local execution model (permissions) | 7 | 7 | Unchanged |
| Filesystem boundary | 5 | **7** | Symlink closed; denylist hardened |
| Data egress controls | 6 | **7** | Broader scanner; still tool-results-only |
| Remote / network surface | 4 | **8** | Unauthenticated tunnel disabled |
| Supply chain (deps + project trust) | 6 | **8** | Project hooks/MCP require trust |
| **Overall** | **6.5** | **8.0** | |

---

## What's left (the honest residual)

1. **Bring `/remote` back with a per-session token** + refuse Auto-mode tunnels. Highest-value next step; the feature is missed while disabled.
2. **Scan outbound user messages**, not just tool results, in the secret pre-flight — and soften the marketing claim to "best-effort."
3. **Consider an opt-in project jail** for file reads when zap runs against untrusted input, layered on top of today's denylist.

---

## Bottom line

v1 said the foundation was strong and the unlocked doors were all at the trust boundary. **Those doors are now shut**: the public tunnel can't be opened, an untrusted repo can't run its own hooks, symlinks can't smuggle a read past the guard, and the session DB is no longer world-readable. The score rises from 6.5 to **8.0** — and every point of that increase is backed by source I re-read and a suite I watched pass (171 tests, 0 failures, 0 warnings). The remaining gap is genuine design residual, documented above and owned, not hidden.

## Disclaimer

This is an AI-assisted source-level security review by Mythos. It catches design- and pattern-level issues through code tracing; it is **not** a substitute for a professional penetration test, dynamic analysis, or a dependency-level supply-chain audit. No live exploitation was performed. For a production deployment handling third-party code or sensitive data, commission a qualified security firm. Use this review to prioritize hardening, not as a certificate of safety.

*Reviewed strictly from `src/**` at version 0.15.11. Build: clean, 0 warnings. Tests: 171 passed, 0 failed.*
