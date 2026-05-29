# Full annotated TUI demo — code indexing, scenario 1
# Output: demo_tui.mp4 (no audio — add voiceover in post with merge_voice.sh)
#
# Flow:
#   1. title card     (6 s)
#   2. scene card     (6 s)
#   3. TUI session    (60 s) — zap starts, query typed, response streams
#   4. note card      (8 s)  — explains what just happened
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

# Type the query into the TUI input area
Type "Where is the Flask class defined?"
Sleep 1200ms
Enter

# Wait: LLM queries index → tool calls stream → response appears
Sleep 55s

# Exit TUI
Ctrl+Q
Sleep 1500ms

# ── 4. Note card ─────────────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "../scenarios/_note_01.sh"
Sleep 400ms
Enter
Sleep 8s
