#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

FLASK_TAG="3.1.0"
REPO_DIR="flask"

if [[ ! -d "$REPO_DIR" ]]; then
  echo "  → cloning pallets/flask $FLASK_TAG …"
  git clone --depth=1 --branch "$FLASK_TAG" https://github.com/pallets/flask.git "$REPO_DIR"
else
  echo "  ✓ flask/ already present"
fi

echo "  → indexing $REPO_DIR …"
cd "$REPO_DIR"
zap --index-only

echo ""
echo "  ✓ ready — run: ./run.sh"
