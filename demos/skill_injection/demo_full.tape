# Full demo — all 3 scenarios (~2.5 min)
# Shows:
#   S1: single skill  →  ↳ skills: python
#   S2: two skills    →  ↳ skills: python, git
#   S3: casual msg    →  no ↳ line  (skill injection skipped entirely)
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
Set TypingSpeed 40ms

Hide
Type `export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"`
Enter
Sleep 200ms
Type "cd flask"
Enter
Ctrl+L
Show

# ── Scenario 1: Python skill ──────────────────────────────────────────────────
Sleep 300ms
Type "../scenarios/01_python_skill.sh"
Sleep 800ms
Enter
Sleep 45s

# ── Scenario 2: Two skills on one turn ────────────────────────────────────────
Sleep 1500ms
Ctrl+L
Sleep 400ms
Type "../scenarios/02_two_skills.sh"
Sleep 800ms
Enter
Sleep 45s

# ── Scenario 3: Casual — no skill injection ───────────────────────────────────
Sleep 1500ms
Ctrl+L
Sleep 400ms
Type "../scenarios/03_casual.sh"
Sleep 800ms
Enter
Sleep 20s
