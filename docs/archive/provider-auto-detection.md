# Provider Auto-Detection & Registration

**Document**: Requirements + Proposal
**Date**: 2026-05-31
**Status**: Draft

---

## Problem Statement

### Current `/provider` UX is broken in TUI

The TUI provider picker (`/provider` slash command) currently:
- Shows a pickable list of 13 providers
- On Enter, silently picks the first model with `api_key: None`
- No API key prompt, no model selection, no "Other…" custom input
- Result: providers that need a key immediately fail on the next API call

The CLI path (via `inquire`) has full step-by-step wizard UX, but the TUI path was left as a stub.

### No credential auto-detection

Users must manually paste API keys even when credentials are already present:
- `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, etc. already exist in their environment
- `gh auth token` is already signed in (GitHub CLI)
- `gcloud auth application-default` is already authenticated

Other tools (Cursor, OpenCode, Zed) auto-detect these — zap should too.

### Missing providers

- **GitHub Copilot** has an OpenAI-compatible API but is not registered
- **Claude Code Pro/Max** is registered but locked behind `coming_soon: true`

---

## Proposed UX

```
User types /provider
    │
    ▼
┌─────────────────────────────────────────────┐
│  switch provider   ↑↓ navigate  Enter select│
├─────────────────────────────────────────────┤
│     LM Studio                    ✓ ready    │
│     Ollama                       ✓ ready    │
│     Anthropic                    ✓ ready    │  ← detected ANTHROPIC_API_KEY
│     OpenAI                       ✓ ready    │  ← detected OPENAI_API_KEY
│     Google Gemini                ✓ ready    │  ← detected gcloud auth
│     GitHub Copilot               ✓ ready    │  ← detected gh auth token
│     Claude Code (Pro/Max)   ◷ coming Jun 16 │
│     DeepSeek                    (no key)    │  ← no DEEPSEEK_API_KEY found
│     Groq                        (no key)    │
│     xAI (Grok)                  (no key)    │
│     ...                                      │
└─────────────────────────────────────────────┘
```

- Providers with detected credentials → **"✓ ready"** → select to switch immediately
- Providers without → **"(no key)"** → selecting prompts for key
- Local providers → probed via quick HTTP request
- Claude Code → **"◷ coming Jun 16"** until auth mechanism is confirmed

---

## Credential Auto-Detection Table

| Provider | Auto-Detect Method | Fallback Env Var |
|---|---|---|
| **GitHub Copilot** | `gh auth token` (reads `~/.config/gh/hosts.yml`) | `GITHUB_TOKEN` |
| **Google Gemini** | `gcloud auth application-default print-access-token` | `GOOGLE_API_KEY` |
| **Anthropic** | — | `ANTHROPIC_API_KEY` |
| **Claude Code** (Jun 16) | Anthropic console OAuth / `claude` CLI session | TBD |
| **OpenAI** | — | `OPENAI_API_KEY` |
| **DeepSeek** | — | `DEEPSEEK_API_KEY` |
| **Groq** | — | `GROQ_API_KEY` |
| **Mistral** | — | `MISTRAL_API_KEY` |
| **xAI** | — | `XAI_API_KEY` |
| **Together** | — | `TOGETHER_API_KEY` |
| **Perplexity** | — | `PERPLEXITY_API_KEY` |
| **Cohere** | — | `COHERE_API_KEY` |
| **LM Studio** | HTTP GET `localhost:1234/v1/models` | — |
| **Ollama** | HTTP GET `localhost:11434` | — |

---

## Implementation Plan

### Phase 1: TUI Provider Wizard (priority: highest)

**Files**: `src/tui/turn_handler.rs`, `src/tui/app.rs`, `src/tui/input.rs`, `src/tui/render/overlays.rs`

- Multi-step TUI wizard inside the provider picker:
  1. Provider list with auto-detected status badges
  2. If no key detected → API key input overlay (masked)
  3. Model selection from list, with "Other…" → custom text input
  4. If `custom` provider → base URL input
  5. Confirm & save to config
- Uses existing TUI patterns: modal overlays, text input handling (`GoalState`, `InitWizardState`, `PermissionPopup`)

### Phase 2: Credential Auto-Detection Module

**Files**: `src/llm_client/auth.rs` (new), `src/tui/turn_handler.rs`

- `check_credentials(slug, env_key, check_fn) -> Option<String>`
- Runs fast checks: env vars, `gh auth token` shell-out, HTTP reachability probe
- Results cached per session — no re-probe on repeated `/provider` openings
- Provider entries get an `env_key` field (data-driven, not a giant match)

### Phase 3: Add GitHub Copilot

**Files**: `src/session/commands/provider.rs`, `src/tui/turn_handler.rs`

- New `github_copilot` provider entry:
  - Kind: `OpenAi`
  - Models: `["gpt-4o", "gpt-4o-mini", "claude-sonnet-4-6", "Other…"]`
  - Base URL: `https://api.githubcopilot.com/chat/completions`
  - Auth: `Bearer <github_token>` (same header format as OpenAI-compatible)

### Phase 4: Activate Claude Code Pro/Max (post-June 16)

**Files**: `src/session/commands/provider.rs`, `src/tui/turn_handler.rs`

- Change `coming_soon: false`
- Add auth mechanism once Anthropic confirms (likely OAuth or console API key tied to subscription)
- If browser OAuth is needed, add a lightweight OAuth flow (open browser, paste callback URL)

### Phase 5 (optional): Auth Method Abstraction

**File**: `src/llm_client/mod.rs`, `src/config.rs`

- Currently all auth is `api_key` → `x-api-key` or `Bearer` header
- If Copilot/Claude Code need different auth, add `auth_method` to `ProviderEntry`
- `create_client` dispatches the right header format

---

## What Already Works

| Provider | Wire Protocol | Notes |
|---|---|---|
| Anthropic (direct) | Anthropic | Works with API key |
| OpenAI | OpenAI | Works with API key |
| Google Gemini | OpenAI | Works with API key |
| DeepSeek | OpenAI | Works with API key |
| Groq | OpenAI | Works with API key |
| Mistral | OpenAI | Works with API key |
| xAI (Grok) | OpenAI | Works with API key |
| Together | OpenAI | Works with API key |
| Perplexity | OpenAI | Works with API key |
| Cohere | OpenAI | Works with API key |
| LM Studio | OpenAI | Works (local, no key) |
| Ollama | OpenAI | Works (local, no key) |
| Custom | OpenAI | Works (any endpoint) |

---

## Open Questions

1. **Claude Code Pro/Max auth**: Exactly how will Anthropic expose API access to Pro/Max subscribers? OAuth flow? Console API key in dashboard? Need to wait for June 16.
2. **Copilot rate limits**: The GitHub Copilot API has rate limits tied to the subscription tier. Should zap show a warning when using Copilot?
3. **Multi-key support**: Should users be able to have multiple keys per provider (e.g. personal + work OpenAI keys)? Deferring — not in scope for this proposal.
