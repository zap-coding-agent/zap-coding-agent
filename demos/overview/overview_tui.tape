# ⚡ zap — TUI Overview
# 3 queries in the TUI, ~2 min
#
# Key insight: the TUI drops Enter while AI is streaming (AppState != Idle).
# So we need generous sleeps after each query to let the LLM finish.
# PlaybackSpeed 1.5 compensates so the video doesn't feel too slow.
#
# Usage: cd demos/overview && VHS_NO_SANDBOX=1 vhs overview_tui.tape

Output overview_tui.mp4
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
Type "cd ../code_indexing/flask"
Enter
Sleep 200ms
Ctrl+L
Show

# ── Title card ─────────────────────────────────────────────────────────────────
Sleep 500ms
Type "../../overview/scenarios/_title.sh"
Sleep 400ms
Enter
Sleep 4s

# ── Launch TUI ─────────────────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "clear"
Enter
Sleep 200ms
Type "AGENT_PERMISSION_MODE=auto zap"
Sleep 600ms
Enter

# Wait for TUI to boot (Session::new, connect to LLM, render initial screen)
# Extra wait in case domain picker appears
Sleep 8s

# ── Query 1: Skill injection ─────────────────────────────────────────────────
# Python question → python skill auto-injects, code index used for lookups
# This may involve tool calls (find_definition, code_map) — needs generous wait
Type "In this Flask project, what Python pattern is used for request lifecycle handling?"
Sleep 800ms
Enter
# Wait for LLM response + possible tool calls to complete (generous: 90s real → 60s video)
Sleep 90s

# ── Query 2: Simple follow-up ─────────────────────────────────────────────────
Type "Where is the Flask class defined? File and line please."
Sleep 600ms
Enter
Sleep 30s

# ── Query 3: Casual turn ──────────────────────────────────────────────────────
Type "Nice, thanks!"
Sleep 400ms
Enter
Sleep 12s

# ── Exit ──────────────────────────────────────────────────────────────────────
Type "/exit"
Sleep 400ms
Enter
Sleep 3s

# ── Wrap-up ───────────────────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/_wrap.sh"
Sleep 400ms
Enter
Sleep 6s
