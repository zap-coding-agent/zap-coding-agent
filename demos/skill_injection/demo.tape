# Hero demo — scenario 1 only (~30 s)
# Shows: Python skill auto-injected → ↳ skills: python  appears before the response
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
Set TypingSpeed 40ms

Hide
Type `export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"`
Enter
Sleep 200ms
Type "cd flask"
Enter
Ctrl+L
Show

Sleep 300ms
Type "../scenarios/01_python_skill.sh"
Sleep 800ms
Enter

# Wait: session startup → skill match → ↳ skills: python → LLM response
Sleep 45s
