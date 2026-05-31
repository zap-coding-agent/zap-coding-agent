# Gemini Login — No API Key (gcloud ADC)

**Document**: Design + Implementation Plan
**Date**: 2026-05-31
**Status**: Draft
**Depends on**: `docs/roadmap/provider-auto-detection.md` (Phase 2)

---

## Problem Statement

### Current state

Gemini is registered as a provider with `kind: OpenAi` and `needs_key: true`. Users must paste a Google API key. This is **not the standard UX** for coding agents — other tools (Cursor, OpenCode, Zed) detect `gcloud` credentials and work without any key prompt.

### Why a static API key doesn't work for Gemini

Google's `generativelanguage.googleapis.com/v1beta/openai/chat/completions` accepts **either**:
- API key → `x-goog-api-key` header or `?key=` query param
- OAuth access token → `Authorization: Bearer <token>` header

The existing `OpenAiClient` sends `Authorization: Bearer {api_key}` — but API keys are NOT valid Bearer tokens on Google's endpoint. So **even a pasted API key currently fails** for Gemini.

The `gcloud` CLI provides OAuth access tokens (`gcloud auth application-default print-access-token`) which ARE valid Bearer tokens, and the `OpenAiClient` already sends Bearer headers. The gap is that tokens are **short-lived (~60 minutes)** and must be **refreshed at request time**, while the current system treats `api_key` as a static string.

### Goal

```
User runs:  gcloud auth login          # one-time browser login
User runs:  zap
> /provider → Google Gemini ✓ ready    # no key prompt, auto-detected
> Select → gemini-2.5-flash            # just works
```

No API key. No copy-paste. Token auto-refreshes.

---

## Design

### New module: `src/llm_client/credentials.rs`

A `CredentialProvider` enum replaces the static `api_key: String` currently threaded through the LLM client stack.

```rust
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub enum CredentialProvider {
    /// Static API key — existing behavior for Anthropic, OpenAI, etc.
    Static(String),

    /// gcloud Application Default Credentials.
    /// Runs `gcloud auth application-default print-access-token` on-demand.
    /// Caches result for 50 minutes (tokens expire after 60 min by default).
    GcloudAdc {
        cached: Mutex<Option<(String, Instant)>>,
    },
}

impl CredentialProvider {
    /// Fetch the credential, refreshing from gcloud if expired.
    pub fn get(&self) -> Result<String> {
        match self {
            Self::Static(key) => Ok(key.clone()),
            Self::GcloudAdc { cached } => {
                let mut lock = cached.lock().unwrap();
                if let Some((token, ts)) = lock.as_ref() {
                    if ts.elapsed() < Duration::from_secs(50 * 60) {
                        return Ok(token.clone()); // cached, still valid
                    }
                }
                // Refresh from gcloud
                let output = std::process::Command::new("gcloud")
                    .args(["auth", "application-default", "print-access-token"])
                    .output()
                    .map_err(|e| format!("gcloud failed: {e}"))?;
                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("gcloud ADC failed: {stderr}"));
                }
                let token = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .to_string();
                *lock = Some((token.clone(), Instant::now()));
                Ok(token)
            }
        }
    }
}
```

### Threading `CredentialProvider` through the stack

| Component | Current | New |
|-----------|---------|-----|
| `OpenAiClient::new()` | Takes `api_key: String` | Takes `credential: CredentialProvider` |
| `AnthropicClient::new()` | Takes `api_key: String` | Takes `credential: CredentialProvider` (Static for now) |
| `create_client()` | Builds from `ProviderEntry.api_key` | Builds `CredentialProvider` from `ProviderEntry` |
| `ProviderEntry` | Has `api_key: Option<String>` | Adds `credential_method: Option<String>` — `"api_key"` (default) or `"gcloud_adc"` |
| `OpenAiClient::send()` | `api_key.clone()` at top | `self.credential.get()?` at top |

### Gemini auth header fix

When using Gemini with gcloud ADC, the existing `Authorization: Bearer <token>` header works correctly — Google's OpenAI-compatible endpoint accepts OAuth Bearer tokens.

When using Gemini with an API key, the header MUST be `x-goog-api-key: <key>`, NOT `Authorization: Bearer <key>`. The existing `OpenAiClient` sends `Authorization: Bearer`. We fix this by allowing `ProviderDef` to specify a custom auth header:

```rust
// In provider.rs
pub struct ProviderDef {
    pub slug: &'static str,
    pub name: &'static str,
    pub kind: ProviderKind,
    pub needs_key: bool,
    pub coming_soon: bool,
    pub base_url: &'static str,
    pub default_model: &'static str,
    pub models: &'static [&'static str],
    pub auth_header: Option<&'static str>,  // NEW: e.g. Some("x-goog-api-key")
}
```

Gemini entry: `auth_header: Some("x-goog-api-key")`. All others: `None` (defaults to `Authorization: Bearer`).

`OpenAiClient` stores `auth_header: String` (default `"Authorization"`), sends `{auth_header}: Bearer {credential}`.

---

## Implementation Phases

### Phase 1: `CredentialProvider` + `auth_header` plumbing

**Files**: `src/llm_client/credentials.rs` (new), `src/llm_client/mod.rs`, `src/llm_client/openai.rs`, `src/llm_client/anthropic.rs`

- Create `CredentialProvider` enum with `Static` and `GcloudAdc` variants
- Add `auth_header: Option<&'static str>` to `ProviderDef`
- Wire both into `OpenAiClient` and `AnthropicClient`
- Update `create_client()` factory

**Verification**: Unit test for `GcloudAdc` token caching/expiry. Integration test: Gemini with API key (via `x-goog-api-key` header) succeeds.

### Phase 2: `ProviderEntry` persistence + config layer

**Files**: `src/config.rs`

- Add `credential_method: Option<String>` to `ProviderEntry`
- Persist to `[providers.<slug>]` in `~/.agent.toml`
- `Config::load()` builds correct `CredentialProvider` variant

**Verification**: Round-trip test: save Gemini with `gcloud_adc`, reload, confirm correct provider variant.

### Phase 3: Provider wizard — Gemini auto-detection

**Files**: `src/llm_client/auth.rs` (new), `src/session/commands/provider.rs`

- `check_gcloud_adc() -> Option<String>` — shells out to `gcloud auth application-default print-access-token`, caches result
- Gemini `ProviderDef` gets `credential_detect: Some(check_gcloud_adc)`
- Provider list shows "✓ ready" when detected, no API key prompt on selection
- If `gcloud` not detected → message: `"Run 'gcloud auth login' first, or paste an API key:"`, fallback to key input

**Verification**: Manual test: run `gcloud auth login`, open `/provider`, see Gemini "✓ ready", select, send message → success.

### Phase 4: TUI provider wizard

**Files**: `src/tui/turn_handler.rs`, `src/tui/app.rs`, `src/tui/input.rs`, `src/tui/render/overlays.rs`

From roadmap Phase 1 — the TUI provider picker is currently a stub. This phase makes the full wizard work in TUI, including auto-detection badges and the Gemini no-key path.

**Verification**: Manual TUI test: open `/provider` in TUI, navigate list with badges, select Gemini (auto-detected), confirm it works.

---

## Files Summary

| File | Action | Purpose |
|------|--------|---------|
| `src/llm_client/credentials.rs` | **New** | `CredentialProvider` enum with `Static` and `GcloudAdc` |
| `src/llm_client/auth.rs` | **New** | `check_gcloud_adc()` auto-detection function |
| `src/llm_client/mod.rs` | Modify | Add `pub mod credentials;`, update `create_client()` |
| `src/llm_client/openai.rs` | Modify | Replace `api_key: String` with `credential: CredentialProvider` + `auth_header` |
| `src/llm_client/anthropic.rs` | Modify | Replace `api_key: String` with `credential: CredentialProvider` |
| `src/config.rs` | Modify | Add `credential_method` to `ProviderEntry`, wire into save/load |
| `src/session/commands/provider.rs` | Modify | Add `auth_header` + `credential_detect` to `ProviderDef`, wizard logic |
| `src/tui/turn_handler.rs` | Modify | Full TUI provider wizard (from roadmap Phase 1) |
| `src/tui/app.rs` | Modify | Provider wizard state machine |
| `src/tui/input.rs` | Modify | Input handling for provider wizard |
| `src/tui/render/overlays.rs` | Modify | Render provider wizard overlays |

---

## What This Does NOT Do

- **No custom OAuth2 browser flow** — `gcloud auth login` already handles that
- **No Vertex AI endpoint** — public `generativelanguage.googleapis.com` is simpler
- **No Gemini CLI integration** — only `gcloud` ADC
- **No multi-key per provider** — one credential per provider slug
- **No `GEMINI_API_KEY` env var** — primary path is `gcloud` ADC; API key is fallback only
