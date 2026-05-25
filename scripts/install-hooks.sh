#!/bin/sh
# Install git hooks for this repo. Run once after cloning.
set -e

REPO_ROOT="$(git rev-parse --show-toplevel)"
HOOKS_DIR="$REPO_ROOT/.git/hooks"

cp "$REPO_ROOT/scripts/pre-commit" "$HOOKS_DIR/pre-commit"
chmod +x "$HOOKS_DIR/pre-commit"

echo "✓ pre-commit hook installed — clippy, file-size, FEATURES.md, and version bump enforced on src/ changes"
