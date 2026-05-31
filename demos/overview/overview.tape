# Overview demo — 5 features, ~4 min
# Flow:
#   1. title card        (8 s)  — what zap is
#   2. skill injection   (46 s) — scene (6s) + execute (40s)
#   3. code index        (36 s) — scene (6s) + execute (30s)
#   4. casual turn       (26 s) — scene (6s) + execute (20s)
#   5. /init             (31 s) — scene (6s) + execute (25s)
#   6. security          (31 s) — scene (6s) + execute (25s)
#   7. wrap-up           (10 s)
#
# Usage (run from demos/overview/):
#   brew install vhs
#   VHS_NO_SANDBOX=1 vhs overview.tape   → overview.mp4

Output overview.mp4
Set FontSize 14
Set Width 1400
Set Height 800
Set Theme "TokyoNight"
Set PlaybackSpeed 0.75
Set TypingSpeed 50ms

Hide
Type `export PATH="$HOME/.local/bin:$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"`
Enter
Sleep 200ms
Type "cd ../code_indexing/flask"
Enter
Ctrl+L
Show

# ── 1. Title card ────────────────────────────────────────────────────────────
Sleep 500ms
Type `../../overview/scenarios/_title.sh`
Sleep 600ms
Enter
Sleep 8s

# ── 2. Skill injection ───────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/_scene_01.sh"
Sleep 400ms
Enter
Sleep 6s

Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/01_skill_injection.sh"
Sleep 600ms
Enter
Sleep 40s

# ── 3. Code index ────────────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/_scene_02.sh"
Sleep 400ms
Enter
Sleep 6s

Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/02_code_index.sh"
Sleep 600ms
Enter
Sleep 30s

# ── 4. Casual turn detection ─────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/_scene_03.sh"
Sleep 400ms
Enter
Sleep 6s

Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/03_casual.sh"
Sleep 600ms
Enter
Sleep 20s

# ── 5. /init bootstrapping ───────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/_scene_04.sh"
Sleep 400ms
Enter
Sleep 6s

Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/04_init.sh"
Sleep 600ms
Enter
Sleep 25s

# ── 6. Security boundary ─────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/_scene_05.sh"
Sleep 400ms
Enter
Sleep 6s

Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/05_security.sh"
Sleep 600ms
Enter
Sleep 25s

# ── 7. Wrap-up ───────────────────────────────────────────────────────────────
Ctrl+L
Sleep 300ms
Type "../../overview/scenarios/_wrap.sh"
Sleep 400ms
Enter
Sleep 10s
