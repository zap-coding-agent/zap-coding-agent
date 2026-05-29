# Full demo — all 3 scenarios (~4 min)
# Flow per scenario: scene explanation (5s) → execute → read output
#
# Usage (run from demos/skill_injection/ after ./setup.sh):
#   brew install vhs
#   VHS_NO_SANDBOX=1 vhs demo_full.tape   → demo_full.gif

Output demo_full.gif
Set FontSize 13
Set Width 1400
Set Height 700
Set Theme "TokyoNight"
Set PlaybackSpeed 1
Set TypingSpeed 60ms

Hide
Type `export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"`
Enter
Sleep 200ms
Type "cd flask"
Enter
Ctrl+L
Show

# ── Title card ────────────────────────────────────────────────────────────────
Sleep 500ms
Type "../scenarios/_title.sh"
Sleep 600ms
Enter
Sleep 6s

# ── Scenario 1: Python skill ──────────────────────────────────────────────────
Ctrl+L
Sleep 400ms
Type "../scenarios/_scene_01.sh"
Sleep 600ms
Enter
Sleep 5s

Ctrl+L
Sleep 400ms
Type "../scenarios/01_python_skill.sh"
Sleep 800ms
Enter
Sleep 50s

# ── Scenario 2: Two skills on one turn ───────────────────────────────────────
Ctrl+L
Sleep 400ms
Type "../scenarios/_scene_02.sh"
Sleep 600ms
Enter
Sleep 5s

Ctrl+L
Sleep 400ms
Type "../scenarios/02_two_skills.sh"
Sleep 800ms
Enter
Sleep 50s

# ── Scenario 3: Casual — no skill injection ───────────────────────────────────
Ctrl+L
Sleep 400ms
Type "../scenarios/_scene_03.sh"
Sleep 600ms
Enter
Sleep 5s

Ctrl+L
Sleep 400ms
Type "../scenarios/03_casual.sh"
Sleep 800ms
Enter
Sleep 20s
