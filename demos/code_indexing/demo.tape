# Quick hero demo — scenario 1 only (~25 s)
# Records the core value prop: find_definition → INDEX hit → exact answer
#
# Usage (run from demos/code_indexing/ after ./setup.sh):
#   brew install vhs
#   VHS_NO_SANDBOX=1 vhs demo.tape   → demo.gif

Output demo.gif
Set FontSize 13
Set Width 1400
Set Height 700
Set Theme "TokyoNight"
Set PlaybackSpeed 1
Set TypingSpeed 40ms

# Navigate into the Flask repo (silently — index already built by setup.sh)
Hide
Type `export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"`
Enter
Sleep 200ms
Type "cd flask"
Enter
Ctrl+L
Show

# Type the command, pause so the viewer can read it, then execute
Sleep 300ms
Type "../scenarios/01_find_class.sh"
Sleep 800ms
Enter

# Wait: session startup → find_definition → INDEX hit → response
Sleep 30s
