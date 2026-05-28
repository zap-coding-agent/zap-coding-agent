#!/usr/bin/env bash
# Clone Flask at a pinned commit and build the zap code index.
set -euo pipefail

DEMO_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$DEMO_DIR/flask"
# Pin to a specific tag so the demo is reproducible.
FLASK_TAG="3.1.0"

# ── 1. Clone ──────────────────────────────────────────────────────────────────
if [ -d "$REPO_DIR/.git" ]; then
    echo "✓ flask/ already cloned"
else
    echo "→ cloning pallets/flask @ $FLASK_TAG …"
    git clone --depth=1 --branch "$FLASK_TAG" \
        https://github.com/pallets/flask.git "$REPO_DIR"
    echo "✓ cloned"
fi

# ── 2. Build the index ────────────────────────────────────────────────────────
echo ""
echo "→ building code index (tree-sitter → SQLite) …"
cd "$REPO_DIR"

# --index-only: tree-sitter scan + SQLite write, no LLM, no session.
zap --index-only

echo ""
echo "✓ setup complete — run ./run.sh to start the demo"
