# Hero demo — scenario 1, fully annotated (~90 s at 0.75x)
# Flow:
#   1. title card     (5 s)  — what this demo is about
#   2. scene card     (5 s)  — what to watch for
#   3. execution      (50 s) — zap runs, output streams
#   4. note card      (7 s)  — explains each line of output
#
# Usage (run from demos/code_indexing/ after ./setup.sh):
#   brew install vhs
#   VHS_NO_SANDBOX=1 vhs demo.tape   → demo.gif

Output demo.gif
Set FontSize 13
Set Width 1400
Set Height 700
Set Theme "TokyoNight"
Set PlaybackSpeed 0.75
Set TypingSpeed 60ms

Hide
Type `export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"`
Enter
Sleep 200ms
Type "cd flask"
Enter
Ctrl+L
Show

# ── 1. Title card ─────────────────────────────────────────────────────────────
Sleep 500ms
Type "../scenarios/_title.sh"
Sleep 400ms
Enter
Sleep 5s

# ── 2. Scene explanation ──────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "../scenarios/_scene_01.sh"
Sleep 400ms
Enter
Sleep 5s

# ── 3. Execute ────────────────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "../scenarios/01_find_class.sh"
Sleep 600ms
Enter
Sleep 50s

# ── 4. Note card — explains what just happened ────────────────────────────────
Type "../scenarios/_note_01.sh"
Sleep 400ms
Enter
Sleep 7s
