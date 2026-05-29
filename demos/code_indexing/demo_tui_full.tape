# Full TUI demo — 3 scenarios (~7 min)
#
#   S1: find_definition → INDEX hit (find Flask class)
#   S2: find_definition × 4 + read_file combo (trace URL routing)
#   S3: code_map → full inheritance chain (Blueprint public API)
#
# Usage (run from demos/code_indexing/ after ./setup.sh):
#   VHS_NO_SANDBOX=1 vhs demo_tui_full.tape   → demo_tui_full.mp4

Output demo_tui_full.mp4
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

# ── Title card ────────────────────────────────────────────────────────────────
Sleep 400ms
Type "../scenarios/_title.sh"
Sleep 400ms
Enter
Sleep 6s

# ══ Scenario 1: find_definition → INDEX hit ═══════════════════════════════════
Ctrl+L
Sleep 300ms
Type "../scenarios/_scene_01.sh"
Sleep 400ms
Enter
Sleep 6s

Ctrl+L
Sleep 300ms
Type "zap"
Enter
Sleep 5s

Type "Where is the Flask class defined?"
Sleep 1200ms
Enter
Sleep 80s

Ctrl+D
Sleep 2s

Type "clear && ../scenarios/_note_01.sh"
Sleep 400ms
Enter
Sleep 7s

# ══ Scenario 2: index-guided cross-file trace ══════════════════════════════════
Ctrl+L
Sleep 300ms
Type "../scenarios/_scene_02_tui.sh"
Sleep 400ms
Enter
Sleep 6s

Ctrl+L
Sleep 300ms
Type "zap"
Enter
Sleep 5s

Type "How does Flask route a URL to its view function? Walk me through the code path."
Sleep 1200ms
Enter
Sleep 120s

Ctrl+D
Sleep 2s

Type "clear && ../scenarios/_note_02.sh"
Sleep 400ms
Enter
Sleep 7s

# ══ Scenario 3: code_map → full inheritance chain ═════════════════════════════
Ctrl+L
Sleep 300ms
Type "../scenarios/_scene_03_tui.sh"
Sleep 400ms
Enter
Sleep 6s

Ctrl+L
Sleep 300ms
Type "zap"
Enter
Sleep 5s

Type "What public methods does the Blueprint class expose? List them with file and line number."
Sleep 1200ms
Enter
Sleep 75s

Ctrl+D
Sleep 2s

Type "clear && ../scenarios/_note_03.sh"
Sleep 400ms
Enter
Sleep 8s
