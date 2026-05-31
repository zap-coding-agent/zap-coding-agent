# zap Live Instance #1: Code Indexing Catches What Claude Code Missed

**Series**: `docs/live-zap-comparison/` — real outputs from real work, unedited.  
**Feature highlighted**: Code indexing (`/init` → `.zap/code.db`)  
**Date**: 2026-05-31  
**Document reviewed**: `docs/roadmap/provider-auto-detection.md`

---

## Setup

A requirements document was written proposing a multi-phase plan to add credential auto-detection to zap's provider picker. Two reviews were done in the same session:

1. **Claude Code** — reviewed the document using its analysis capabilities. Has file access via tools (Read, Bash, grep) but no pre-built index and did not query source files during this review.
2. **zap** (DeepSeek V4 Pro) — reviewed the same document with code indexing active. Its system prompt instructs it to cross-reference proposals against `.zap/code.db` automatically.

---

## Step 1 — Claude Code's review (verbatim)

> **Issue 1: Google Gemini auth is wrong (significant)**
>
> Line 72 lists `gcloud auth application-default print-access-token` for Gemini. That's Vertex AI auth, not the Gemini API. The "What Already Works" table shows Gemini uses the OpenAI wire protocol — that's the AI Studio API, which authenticates via `GOOGLE_API_KEY` (not gcloud). Running `gcloud` subprocess would give the wrong token type and would fail or silently not work for users without the gcloud CLI. Change the Gemini row to just:
> ```
> | **Google Gemini** | — | `GOOGLE_API_KEY` or `GEMINI_API_KEY` |
> ```
>
> **Issue 2: `check_credentials` return type is underspecified**
>
> Phase 2 proposes `check_credentials(slug, env_key, check_fn) -> Option<String>`. For local providers (LM Studio, Ollama) there's no credential string — just reachability. `Option<String>` conflates two very different concepts. The implementation will immediately hit this mismatch. Suggest:
> ```rust
> enum CredStatus { Ready, NoKey, Unreachable }
> ```
> or a simple `bool is_available` + `Option<String> key` pair.
>
> **Issue 3: Key persistence is unspecified**
>
> Phase 1 says "Confirm & save to config" but doesn't say *where*. Currently API keys live in `~/.agent.toml`. The wizard needs to explicitly state: write to `~/.agent.toml`, just like the CLI wizard does. If the user enters a key but it's session-only, it will vanish and they'll be confused. This decision belongs in the requirements, not left to implementation.
>
> **Issue 4: TUI wizard complexity underestimated**
>
> Phase 1 lists 5 wizard steps as "highest priority." That's the largest single UI task in the plan and touches 4 files plus new overlay states. If the real goal is just auto-detection, Phase 2 (detect + show badges) delivers more user value with less risk. Consider reordering: Phase 1 = detection + badge display, Phase 2 = full wizard. Auto-detection alone makes the picker much more useful even without the wizard.
>
> **Issue 5: GitHub Copilot API requires an extra header**
>
> Line 117: the Copilot endpoint accepts OpenAI-format requests, but it also requires `editor-version: vscode/1.85.0` (or similar) in the header, otherwise requests are rejected. The current `create_client` function likely doesn't support per-provider custom headers. Phase 5 (auth abstraction) may be required before Phase 3 can actually work.
>
> **Issue 6 (minor): The "Jun 16" Claude Code date**
>
> The "Jun 16" Claude Code date on line 52 and 161 — is this from an actual Anthropic announcement or speculation? If speculation, mark it as `TBD` rather than a specific date so it doesn't become a stale promise in the UI.
>
> **Minor**: HTTP probe timeout is unspecified — a hung LM Studio/Ollama probe would block picker open. Needs an explicit 500ms or 1s timeout.

---

## Step 2 — zap's review (verbatim, unedited from TUI output)

> **Code Review: `docs/roadmap/provider-auto-detection.md`**
>
> **Summary**
> The proposal correctly diagnoses the two core problems (broken TUI provider flow, no credential auto-detection) and the UX vision is solid. The phasing is sensible — Phase 1 is the right MVP. However, there are several technical gaps that need attention before implementation.
>
> ---
>
> **Blocking Issues**
>
> **1. Provider list duplication is not addressed**
> The same 13-provider list exists in two places — `src/session/commands/provider.rs:22-36` (CLI) and `src/tui/turn_handler.rs:76-91` (TUI). Phase 1 should start by extracting a single `PROVIDERS: &[ProviderDef]` constant. Every phase modifies "both files" — that means duplicating every change (Copilot addition, Claude Code activation, env_key additions). This is the single biggest maintainability risk in the current code and the proposal adds more fields without fixing the root cause.
> → blocking: Add a Phase 0: extract provider definitions to a shared constant.
>
> **2. Copilot auth: `gh auth token` may not work**
> The GitHub Copilot API does not use the generic gh CLI token. It requires a token obtained via the OAuth device flow with a `copilot` or `read:user` scope. The standard `gh auth token` typically has `repo`, `read:org`, `workflow` scopes — these won't authenticate against `api.githubcopilot.com`. The Copilot extension (`gh copilot`) uses a separate auth mechanism.
> → blocking: Verify with GitHub's Copilot API docs before committing to `gh auth token`. Likely needs a dedicated device flow or at minimum a manual token prompt with instructions.
>
> **3. Shell-out timeouts are unspecified**
> `gh auth token` and `gcloud auth application-default print-access-token` are blocking shell calls. gcloud in particular can take 1-3 seconds on first call, and will hang indefinitely if there's a network issue or expired credentials requiring interactive re-auth. The proposal says "Runs fast checks" but gives no timeout, caching strategy, or fallback.
> → blocking: Each shell-out needs a timeout (≤2s). If expired/non-interactive, fall back to env var only. Results must be cached for the session to avoid re-probing on repeated `/provider` opens.
>
> ---
>
> **Important Design Concerns**
>
> **4. "✓ ready" is misleading for env-var-only checks**
> Having `OPENAI_API_KEY` set doesn't mean the key is valid. A revoked, expired, or quota-exhausted key will still show "✓ ready" and then fail on first API call. The proposal should distinguish:
> - `✓ detected` (credential found, untested) for env-var-only providers
> - `✓ ready` only for providers that pass a lightweight auth check (e.g., Copilot via `gh auth status`, Ollama via HTTP probe)
> - `(no key)` for providers without credentials
>
> Alternatively, accept the false-positive risk for env-var providers and document it as "best-effort."
>
> **5. Ollama probe URL**
> The proposal says `HTTP GET localhost:11434`. The root path returns HTML in recent Ollama versions. Use `localhost:11434/api/tags` instead — it's a lightweight JSON endpoint, always available regardless of loaded models.
>
> **6. `env_key` field on ProviderEntry is insufficient**
> For Copilot and Gemini the "auto-detect method" is a script/function, not an env var lookup. The `check_credentials(slug, env_key, check_fn) -> Option<String>` signature is fine but the proposal implies a simple `env_key` field on `ProviderEntry` will handle everything. You need an enum:
>
> ```rust
> enum AuthSource {
>     EnvVar(&'static str),          // OPENAI_API_KEY
>     ShellCommand(&'static str),    // gh auth token
>     HttpProbe(&'static str),       // GET localhost:11434/api/tags
>     OAuth { ... },                 // future
>     Manual,                        // prompt user
> }
> ```
>
> This ties into Phase 5 — I'd recommend moving the auth abstraction to Phase 2 so you're not retrofitting it after adding Copilot and Claude Code.

---

## Step 3 — "Is there anything zap picked that you missed?"

The user asked Claude Code directly. Claude Code's response:

> Yes — three things:
>
> **1. Provider list duplication (Phase 0)** — this was my cleanest miss. I never flagged that the same 13-provider list lives in both `provider.rs` and `turn_handler.rs`. Zap caught that every proposed change would need to be made twice, and that extracting a shared constant should be Phase 0. I didn't mention this at all.
>
> **2. Ollama probe URL** — I flagged "HTTP probe timeout is unspecified" but didn't catch that `localhost:11434` is the wrong target. Zap specifically identified `/api/tags` as the correct lightweight endpoint. I was half-right (timeout concern) but missed the URL being wrong.
>
> **3. AuthSource enum specificity** — I said `Option<String>` was wrong and suggested a richer return type, but I was vague about the shape. Zap went further and proposed the concrete enum (`EnvVar`, `ShellCommand`, `HttpProbe`, `OAuth`, `Manual`) and drew the right conclusion that this should move to Phase 2 before Copilot is added. My version said "needs a richer type" — zap's version is actionable.
>
> The duplication one is the most meaningful miss. It's an architectural issue that affects the whole implementation plan, not just a detail.

---

## Step 4 — What Claude Code caught that zap missed

> **1. Google Gemini auth is wrong** — zap didn't flag this. The doc lists `gcloud auth application-default print-access-token` for Gemini but that's Vertex AI auth. zap uses the Gemini API directly (AI Studio, OpenAI wire protocol) which takes `GOOGLE_API_KEY`. The gcloud token is the wrong type entirely.
>
> **2. GitHub Copilot requires an extra HTTP header** — the Copilot API needs `editor-version: vscode/1.85.0`. `create_client` doesn't support per-provider custom headers today. Phase 5 is a prerequisite for Phase 3, not optional.
>
> **3. Key persistence unspecified** — zap didn't ask where a key entered via the wizard gets saved. Must be `~/.agent.toml` to survive restart.
>
> **4. Jun 16 date is unverified** — hardcoded date in both the UX diagram and open questions. If speculative, it becomes a stale promise in the UI.

---

## What explains the difference

| Finding | Who caught it | Why |
|---|---|---|
| Provider list duplication | zap | Queried code index — found exact file+line locations of both copies. Claude Code had no index and didn't grep. |
| Ollama probe URL wrong | zap | Factual model knowledge about the Ollama API. Claude Code flagged timeout but not the wrong URL. |
| AuthSource enum (concrete design) | zap | More specific reasoning on the same concern Claude Code raised vaguely. |
| Gemini auth wrong (gcloud ≠ AI Studio) | Claude Code | Factual model knowledge about Google's two separate API surfaces. zap didn't catch this. |
| Copilot extra header requirement | Claude Code | Factual model knowledge about the Copilot API. |
| Key persistence gap | Claude Code | Document analysis — both could have caught it; zap didn't. |
| Jun 16 date unverified | Claude Code | Document analysis. |

**The provider list duplication** — the most architecturally significant finding — required knowing the actual state of the code. Claude Code has file access but no pre-built index and didn't look. zap had the index, was instructed to use it, and cited exact line numbers. That's the difference code indexing makes.

---

## What `/init` builds

```
zap /init
```

Walks the project, parses every `.rs`, `.py`, `.ts`, `.go`, `.java`, `.js` file with tree-sitter, and stores symbols (name, kind, file, line, signature) in `.zap/code.db`. Updated in the background every 2 minutes during a session.

During a review, zap can query:
```sql
SELECT path, line, kind, signature
FROM symbols
WHERE name LIKE '%ProviderEntry%';
```
and get real file locations — not guesses.

---

*Next in series: `02-...` — another zap capability, another live instance.*
