#!/usr/bin/env bash
# deploy.sh — build zap (release) and install to both PATH locations with codesign.
#
# Usage:
#   ./scripts/deploy.sh          # release build + install + sign
#   ./scripts/deploy.sh --check  # show installed versions without building
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$REPO/target/release/zap"
DEST1="$HOME/.cargo/bin/zap"
DEST2="$HOME/.local/bin/zap"

# ── Helpers ───────────────────────────────────────────────────────────────────
ok()   { printf "  \033[32m✓\033[0m %s\n" "$*"; }
info() { printf "  \033[36m◌\033[0m %s\n" "$*"; }
warn() { printf "  \033[33m⚠\033[0m %s\n" "$*"; }
die()  { printf "  \033[31m✗\033[0m %s\n" "$*" >&2; exit 1; }

sign_if_macos() {
    local path="$1"
    if [[ "$(uname)" == "Darwin" ]]; then
        codesign --sign - --force "$path" 2>/dev/null \
            && ok "signed  $path" \
            || warn "codesign failed for $path (non-fatal on older macOS)"
    fi
}

# ── --check mode ──────────────────────────────────────────────────────────────
if [[ "${1:-}" == "--check" ]]; then
    for loc in "$DEST1" "$DEST2"; do
        if [[ -x "$loc" ]]; then
            ver=$("$loc" --version 2>/dev/null || echo "?")
            info "$loc  →  $ver"
        else
            warn "$loc  →  not installed"
        fi
    done
    exit 0
fi

# ── Build ─────────────────────────────────────────────────────────────────────
info "building zap (release)…"
cd "$REPO"
cargo build --release 2>&1 | tail -3
[[ -x "$BIN" ]] || die "build output not found at $BIN"
ok "built   $BIN  ($(du -sh "$BIN" | cut -f1))"

# ── Install ───────────────────────────────────────────────────────────────────
for dest in "$DEST1" "$DEST2"; do
    dir="$(dirname "$dest")"
    mkdir -p "$dir"
    cp "$BIN" "$dest"
    ok "copied  $dest"
    sign_if_macos "$dest"
done

# ── Smoke test ────────────────────────────────────────────────────────────────
info "smoke test…"
"$DEST2" --help >/dev/null 2>&1 \
    && ok "zap — ready (both locations updated)" \
    || die "smoke test failed — binary may not be signed correctly (macOS Tahoe requires codesign)"
