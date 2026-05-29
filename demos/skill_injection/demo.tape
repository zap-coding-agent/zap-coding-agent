# Hero demo — scenario 1 only (~1 min)
# Flow: title card (5s) → scene explanation (5s) → execute → read output
#
# Usage (run from demos/skill_injection/ after ./setup.sh):
#   brew install vhs
#   VHS_NO_SANDBOX=1 vhs demo.tape   → demo.gif

Output demo.gif
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

# ── Scenario 1 explanation ────────────────────────────────────────────────────
Ctrl+L
Sleep 400ms
Type "../scenarios/_scene_01.sh"
Sleep 600ms
Enter
Sleep 5s

# ── Execute scenario 1 ────────────────────────────────────────────────────────
Ctrl+L
Sleep 400ms
Type "../scenarios/01_python_skill.sh"
Sleep 800ms
Enter

# Wait: session startup → skill match → ↳ skills: python → LLM response
# Then pause so viewer can read the output
Sleep 50s
