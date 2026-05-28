# Full demo — all 3 scenarios (~2 min)
# Shows: symbol lookup, cross-file request trace, Blueprint API surface
#
# Usage (run from demos/code_indexing/ after ./setup.sh):
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

# ── Scenario 1: find the Flask class ─────────────────────────────────────────
Sleep 300ms
Type "../scenarios/01_find_class.sh"
Sleep 800ms
Enter
Sleep 30s

# ── Scenario 2: trace a request ──────────────────────────────────────────────
Sleep 1500ms
Ctrl+L
Sleep 400ms
Type "../scenarios/02_trace_request.sh"
Sleep 800ms
Enter
Sleep 60s

# ── Scenario 3: Blueprint API surface ────────────────────────────────────────
Sleep 1500ms
Ctrl+L
Sleep 400ms
Type "../scenarios/03_blueprint_api.sh"
Sleep 800ms
Enter
Sleep 60s
