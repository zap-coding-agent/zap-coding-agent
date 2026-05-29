# Hero TUI demo — scenario 1 only (~2 min)
# Flow: title (6s) → scene (6s) → TUI session (80s) → note (8s)
#
# Usage (run from demos/code_indexing/ after ./setup.sh):
#   VHS_NO_SANDBOX=1 vhs demo_tui.tape   → demo_tui.mp4

Output demo_tui.mp4
Set Width 1400
Set Height 820
Set FontSize 13
Set Theme "TokyoNight"
Set PlaybackSpeed 1
Set TypingSpeed 70ms

Hide
Type `export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"`
Enter
Sleep 300ms
Type "cd flask"
Enter
Ctrl+L
Show

# ── 1. Title card ─────────────────────────────────────────────────────────────
Sleep 400ms
Type "../scenarios/_title.sh"
Sleep 400ms
Enter
Sleep 6s

# ── 2. Scene explanation ──────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "../scenarios/_scene_01.sh"
Sleep 400ms
Enter
Sleep 6s

# ── 3. TUI session ────────────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "zap"
Enter
Sleep 5s

Type "Where is the Flask class defined?"
Sleep 1200ms
Enter
Sleep 80s

# Exit TUI cleanly
Ctrl+D
Sleep 2s

# ── 4. Note card (clear first so TUI frame doesn't linger) ───────────────────
Type "clear && ../scenarios/_note_01.sh"
Sleep 400ms
Enter
Sleep 8s
