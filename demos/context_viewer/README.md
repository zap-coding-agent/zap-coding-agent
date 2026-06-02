# context_viewer demo

Records the `/context` overlay feature for the ep08 video.

## Pre-flight

```bash
# 1. zap binary on PATH and codesigned
which zap
codesign -s - -f $(which zap)

# 2. VHS installed
brew install vhs

# 3. Demo project present (reuses the existing Flask project)
ls demos/code_indexing/flask/
```

## Record

Run from the **repo root**:

```bash
VHS_NO_SANDBOX=1 vhs demos/context_viewer/context_viewer.tape
```

Output: `demos/context_viewer/context_viewer.mp4`

## What it records

| Segment | What happens |
|---|---|
| Title card | Static scene — what the feature is |
| TUI turn 1 | "Give me a quick overview…" — LLM reads project structure |
| TUI turn 2 | "Show me all route handlers…" — heavy tool calls, large token cost |
| TUI turn 3 | "What does the User model look like?" — reads model files |
| TUI turn 4 | "Got it, thanks." — casual, near-zero cost (contrast) |
| /context open | Overlay appears with all 4 turns, token costs, ░▓ column |
| Navigate + drop | `j` to Turn 2 → `d` drops it → header token count drops live |
| Compact | `c` — remaining history summarised, bar resets |
| Wrap card | Feature summary + GitHub link |

## Timing notes

- LLM response sleeps are generous (50–65s real time). At 1.5× playback ≈ 35–45s video.
- If responses are faster than expected the tape still works — extra sleep is trimmed at edit time.
- The money shot is the token count dropping after `d` — VHS renders this in real time.

## Adjusting sleeps

If your LLM is slower (local model) increase the sleeps after each `Enter`:
- Turn 1: `Sleep 55s` → `Sleep 90s`
- Turn 2: `Sleep 65s` → `Sleep 120s`
- Turn 3: `Sleep 50s` → `Sleep 80s`
