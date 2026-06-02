# ⚡ zap — /context viewer demo
#
# Timing (at 1.5x playback):
#   Title card     40s  ← room for full hook narration
#   Phase card     10s  ← brief setup
#   TUI + 4 turns  ~60s ← trimmed, just enough to see turns happening
#   /context UI    ~75s ← the focus: list, detail, drop, navigate remains
#   Summary card   ~20s
#   Total          ~3:30
#
# Pre-flight:
#   - zap binary must be on PATH and codesigned
#   - Run from repo root: VHS_NO_SANDBOX=1 vhs demos/context_viewer/context_viewer.tape

Output demos/context_viewer/context_viewer.mp4

Set FontSize 14
Set Width 1400
Set Height 800
Set Theme "TokyoNight"
Set PlaybackSpeed 1.5
Set TypingSpeed 40ms

Hide
Type `export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"`
Enter
Sleep 200ms
Type "cd demos/code_indexing/flask"
Enter
Sleep 200ms
Ctrl+L
Show

# ── 1. Title card  (60s rec → 40s video) ──────────────────────────────────────
Sleep 500ms
Type "../../context_viewer/scenarios/_title.sh"
Sleep 400ms
Enter
Sleep 60s

# ── 2. Phase card  (15s rec → 10s video) ──────────────────────────────────────
Ctrl+L
Sleep 300ms
Type `../../context_viewer/scenarios/_phase.sh "Step 1" "Building real context" "We send 4 questions to a real Flask codebase." "Each question triggers tool calls — file reads and searches — that fill the context window." "Then we open /context to see exactly what is inside and take control."`
Sleep 400ms
Enter
Sleep 15s

# ── 3. Launch TUI ─────────────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "AGENT_PERMISSION_MODE=auto zap"
Sleep 600ms
Enter
Sleep 8s

# ── 4. Build context: 4 turns  (trimmed — just show it happening) ─────────────

# Turn 1 — overview (25s rec → ~17s video)
Type "Give me a quick overview of this Flask project — what it does and how it's structured."
Sleep 600ms
Enter
Sleep 25s

# Turn 2 — route handlers: heaviest turn (35s rec → ~23s video)
Type "Show me all the route handlers and what HTTP methods they handle."
Sleep 600ms
Enter
Sleep 35s

# Turn 3 — data model (25s rec → ~17s video)
Type "What does the User model look like? Fields and relationships."
Sleep 600ms
Enter
Sleep 25s

# Turn 4 — greeting: near-zero cost, contrast turn (10s rec → ~7s video)
Type "Got it, thanks."
Sleep 400ms
Enter
Sleep 10s

# ── 5. Open /context overlay ──────────────────────────────────────────────────
Sleep 2s
Type "/context"
Sleep 500ms
Enter
Sleep 2s

# Pause: viewer reads the header — total tokens, % fill, per-turn breakdown
Sleep 5s

# ── 6. Navigate to Turn 2 ─────────────────────────────────────────────────────
Type "j"
Sleep 800ms
Type "j"
Sleep 800ms

# Pause: Turn 2 highlighted — viewer reads its token cost + % share
Sleep 5s

# ── 7. Open detail panel ──────────────────────────────────────────────────────
Type "l"
Sleep 1s

# Pause: detail opens — user message text visible at top
Sleep 5s

# Scroll down naturally — user text section
Type "j"
Sleep 600ms
Type "j"
Sleep 600ms
Type "j"
Sleep 800ms

# Pause: viewer reads user message fully
Sleep 3s

# Continue scroll into tool calls with JSON input
Type "j"
Sleep 600ms
Type "j"
Sleep 600ms
Type "j"
Sleep 600ms
Type "j"
Sleep 800ms

# Pause: viewer reads tool call names + JSON input (money shot)
Sleep 5s

# Continue scroll into tool results (full file contents)
Type "j"
Sleep 600ms
Type "j"
Sleep 600ms
Type "j"
Sleep 600ms
Type "j"
Sleep 600ms
Type "j"
Sleep 800ms

# Pause: viewer reads tool result content — "every byte is costing you"
Sleep 5s

# Continue scroll to assistant response
Type "j"
Sleep 600ms
Type "j"
Sleep 600ms
Type "j"
Sleep 800ms

# Pause: viewer reads assistant response
Sleep 4s

# ── 8. Return to list ─────────────────────────────────────────────────────────
Type "h"
Sleep 1s

# Pause: Turn 2 highlighted — narrator explains it's no longer needed
Sleep 5s

# ── 9. Drop Turn 2 — confirmation then confirm ────────────────────────────────
# d → row goes red + confirmation banner
Type "d"

# Pause: viewer reads red banner "Drop Turn 2 (Xk, 52%)? [Enter] confirm"
Sleep 6s

# Enter → turn drops, header token count updates instantly
Enter

# Pause: viewer sees the header shrink — 22% → ~10%
Sleep 5s

# ── 9b. Navigate remaining turns — show what's left after the drop ────────────
# Move up to Turn 1 and show its cost
Type "k"
Sleep 1s
Sleep 3s

# Move down to Turn 3
Type "j"
Sleep 1s
Sleep 3s

# Move down to Turn 4 (tiny — the greeting)
Type "j"
Sleep 1s
Sleep 3s

# Open detail of one remaining turn to show it's intact
Type "l"
Sleep 1s
Sleep 4s

# Return to list
Type "h"
Sleep 1s
Sleep 3s

# ── 10. Exit TUI ─────────────────────────────────────────────────────────────
# q / Escape to close context viewer, then /exit
Type "q"
Sleep 1s
Type "/exit"
Sleep 400ms
Enter

# Give terminal time to fully restore
Sleep 8s

# ── 12. Summary card  (30s rec → ~20s video) ──────────────────────────────────
Ctrl+L
Sleep 500ms
Type "../../context_viewer/scenarios/_summary.sh"
Sleep 400ms
Enter
Sleep 30s
