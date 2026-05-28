# Full demo — all 3 scenarios (~2 min)
# Shows: symbol lookup, cross-file request trace, Blueprint API surface
#
# Usage:
#   brew install vhs
#   cd demos/code_indexing
#   vhs demo_full.tape       → produces demo_full.gif

Output demo_full.gif

Set FontSize 13
Set Width 110
Set Height 38
Set Theme "Tokyo Night"
Set PlaybackSpeed 1
Set TypingSpeed 40ms

Hide
Type "cd flask"
Enter
Ctrl+L
Show

# ── Scenario 1: find the Flask class ─────────────────────────────────────────
Sleep 400ms
Type "../scenarios/01_find_class.sh"
Sleep 600ms
Enter
Sleep 30s

# ── Scenario 2: trace a request ──────────────────────────────────────────────
Sleep 1500ms
Ctrl+L
Sleep 400ms
Type "../scenarios/02_trace_request.sh"
Sleep 600ms
Enter
Sleep 60s

# ── Scenario 3: Blueprint API surface ────────────────────────────────────────
Sleep 1500ms
Ctrl+L
Sleep 400ms
Type "../scenarios/03_blueprint_api.sh"
Sleep 600ms
Enter
Sleep 60s
