# Quick hero demo — scenario 1 only (~25 s)
# Records the core value prop: find_definition → INDEX hit → exact answer
#
# Usage:
#   brew install vhs
#   cd demos/code_indexing
#   vhs demo.tape            → produces demo.gif
#
# Embed in README:
#   ![code indexing demo](demos/code_indexing/demo.gif)

Output demo.gif

Set FontSize 13
Set Width 110
Set Height 36
Set Theme "Tokyo Night"
Set PlaybackSpeed 1
Set TypingSpeed 40ms

# ── setup ────────────────────────────────────────────────────────────────────
# Assume flask/ is already cloned and indexed by setup.sh.
# The tape starts at demos/code_indexing/ — navigate into the repo.
Hide
Type "cd flask"
Enter
Ctrl+L
Show

# ── run the demo ──────────────────────────────────────────────────────────────
Sleep 300ms
Type "../scenarios/01_find_class.sh"
Sleep 600ms
Enter

# Wait for: session startup → find_definition tool call → INDEX hit → response
Sleep 30s
