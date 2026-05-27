# Security — A First-Class Concern

zap handles your source code, credentials, and shell — so it treats security as a core feature, not an afterthought.

## Permission modes

In the default `ask` mode, every write operation and shell command is blocked until you approve it. Read-only tools run freely. Only tools that can cause damage require your sign-off:

| Tool class | Ask mode | Auto mode | Deny mode |
|---|---|---|---|
| `read_file`, `search_code`, `code_map`, `git_status` | ✓ always allowed | ✓ | ✗ blocked |
| `edit_file`, `write_file`, `batch_edit` | prompt | ✓ | ✗ |
| `shell` | prompt | ✓ | ✗ |
| `spawn_agent` | prompt | ✓ | ✗ |

When the model wants to run multiple tools in one turn, zap shows **one grouped prompt** covering all of them — you approve or deny the batch, not each individually.

**"Always" grants** — type `always` once at a prompt and that tool class is auto-approved for the rest of the session. Granting `edit_file` also grants `write_file` and `batch_edit` — semantically identical operations share a grant class so you're not re-prompted for the same action with a different tool name.

**Three modes, your choice:**

| Mode | When to use |
|---|---|
| `ask` *(default)* | Any interactive session — you stay in control |
| `auto` | Sandboxed CI, scripts, or headless runs where you control the environment |
| `deny` | Completely read-only — the agent can read and reason but cannot write a single byte or run any command |

Switch at any time: `/permissions ask`, `/permissions auto`, `/permissions deny`.

## Secret scanner — 25+ patterns, blocks before sending

Before any content is sent to a cloud LLM, zap scans it for secrets. It checks for:

- **API keys**: Anthropic (`sk-ant-`), OpenAI (`sk-proj-`), Stripe live and test keys
- **VCS tokens**: GitHub personal access tokens (`ghp_`, `ghs_`, `github_pat_`), GitLab tokens (`glpat-`)
- **Cloud credentials**: AWS access keys (`AKIA`), AWS secret key fields, GCP service account JSON
- **Cryptographic material**: PEM private key blocks (`-----BEGIN`), JWT tokens (base64 header prefix)
- **Generic credential fields**: `password=`, `api_key=`, `secret=`, `access_token=` in config files

Matches are blocked and you're warned with the line number and a redacted preview — content is never silently forwarded.

## Full audit trail

Every tool call is appended to `~/.zap/audit.jsonl` as a structured JSON record with a timestamp, tool name, and outcome. You have a complete, machine-readable record of everything the agent did — useful for debugging, compliance, or just reviewing what changed.

```bash
/audit 20       # show last 20 audit entries in the TUI
```

## Undo for every edit

Before modifying any file, zap snapshots the previous content in memory. If the agent makes a wrong edit, you can restore it instantly:

```
/undo src/main.rs      # restore file to its pre-edit state
```

The model can also undo its own edits via the `undo_edit` tool — useful in autonomous `/goal` runs where the agent detects it made a mistake mid-task.
