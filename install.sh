#!/usr/bin/env bash
set -euo pipefail

REPO="zap-coding-agent/zap-coding-agent"
BIN_NAME="zap"
INSTALL_DIR="${ZAP_INSTALL_DIR:-$HOME/.local/bin}"

# ── Colours ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; RESET='\033[0m'

info()  { printf "${CYAN}  →${RESET} %s\n" "$*"; }
ok()    { printf "${GREEN}  ✓${RESET} %s\n" "$*"; }
warn()  { printf "${YELLOW}  ⚠${RESET} %s\n" "$*"; }
die()   { printf "${RED}  ✗${RESET} %s\n" "$*" >&2; exit 1; }
header(){ printf "\n${BOLD}%s${RESET}\n" "$*"; }

# ── Platform detection ────────────────────────────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64)  ARTIFACT="zap-macos-arm64.tar.gz" ;;
      x86_64) die "macOS Intel (x86_64) builds are not yet available. Build from source: https://github.com/$REPO" ;;
      *)      die "Unknown macOS architecture: $ARCH" ;;
    esac
    ;;
  Linux)
    case "$ARCH" in
      x86_64|amd64)  ARTIFACT="zap-linux-x86_64.tar.gz" ;;
      aarch64|arm64) ARTIFACT="zap-linux-arm64.tar.gz" ;;
      *)             die "Unknown Linux architecture: $ARCH" ;;
    esac
    ;;
  *)
    die "Unsupported OS: $OS. For Windows, see: https://github.com/$REPO/releases/latest"
    ;;
esac

header "Installing zap"

# ── Local binary detection (extracted package) ────────────────────────────────
# When install.sh is run from an extracted release package, the binary sits
# next to this script. Detect that and skip the network download entirely.
# Falls back to GitHub download when piped via curl or run standalone.
BINARY=""
SCRIPT_DIR=""
VERSION="unknown"

# BASH_SOURCE[0] is empty/"-" when piped through stdin (curl|bash).
if [[ -n "${BASH_SOURCE[0]:-}" ]] && [[ "${BASH_SOURCE[0]}" != "-" ]]; then
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || true)"
fi

if [[ -n "$SCRIPT_DIR" ]] && [[ -f "$SCRIPT_DIR/$BIN_NAME" ]]; then
  # ── Extracted-package path: use local binary ──────────────────────────────
  BINARY="$SCRIPT_DIR/$BIN_NAME"
  chmod +x "$BINARY"
  # Try to read version from binary; fall back gracefully if it fails.
  VERSION="$("$BINARY" --version 2>/dev/null | head -1 || echo "local")"
  info "Using local binary: $BIN_NAME  ($VERSION)"
else
  # ── Download path: fetch from GitHub releases ─────────────────────────────
  info "Detecting latest release…"

  if command -v curl &>/dev/null; then
    FETCH="curl -fsSL"
  elif command -v wget &>/dev/null; then
    FETCH="wget -qO-"
  else
    die "curl or wget is required"
  fi

  RELEASE_JSON=$($FETCH "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null) \
    || die "Failed to reach GitHub API — check your internet connection"

  # Extract version and download URL from JSON without requiring jq.
  VERSION=$(printf '%s' "$RELEASE_JSON" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
  DOWNLOAD_URL=$(printf '%s' "$RELEASE_JSON" \
    | grep '"browser_download_url"' \
    | grep "$ARTIFACT" \
    | head -1 \
    | sed 's/.*"browser_download_url": *"\([^"]*\)".*/\1/')

  [[ -n "$VERSION"      ]] || die "Could not determine latest release version"
  [[ -n "$DOWNLOAD_URL" ]] || die "No download found for $ARTIFACT in release $VERSION"

  info "Found $VERSION ($ARTIFACT)"

  TMP="$(mktemp -d)"
  trap 'rm -rf "$TMP"' EXIT

  ARCHIVE="$TMP/$ARTIFACT"
  info "Downloading…"
  if command -v curl &>/dev/null; then
    curl -fsSL --progress-bar "$DOWNLOAD_URL" -o "$ARCHIVE"
  else
    wget -q --show-progress "$DOWNLOAD_URL" -O "$ARCHIVE"
  fi
  ok "Downloaded"

  tar -xzf "$ARCHIVE" -C "$TMP"
  BINARY="$TMP/$BIN_NAME"
  [[ -f "$BINARY" ]] || die "Binary '$BIN_NAME' not found in archive"
  chmod +x "$BINARY"
fi

# ── macOS codesign (required on macOS Tahoe 26.x+) ───────────────────────────
if [[ "$OS" == "Darwin" ]]; then
  if codesign --sign - "$BINARY" &>/dev/null; then
    ok "Code-signed (required on macOS 26+)"
  else
    warn "codesign failed — binary may be blocked on macOS 26+. Run: codesign --sign - $INSTALL_DIR/$BIN_NAME"
  fi
fi

# ── Install ───────────────────────────────────────────────────────────────────
mkdir -p "$INSTALL_DIR"
mv "$BINARY" "$INSTALL_DIR/$BIN_NAME"
ok "Installed → $INSTALL_DIR/$BIN_NAME"

# ── PATH check ────────────────────────────────────────────────────────────────
if ! echo ":$PATH:" | grep -q ":$INSTALL_DIR:"; then
  warn "$INSTALL_DIR is not in your PATH"
  # Detect shell config file.
  SHELL_RC=""
  case "${SHELL:-}" in
    */zsh)  SHELL_RC="$HOME/.zshrc" ;;
    */bash) SHELL_RC="${BASH_PROFILE:-$HOME/.bashrc}" ;;
  esac
  if [[ -n "$SHELL_RC" ]]; then
    LINE="export PATH=\"$INSTALL_DIR:\$PATH\""
    if ! grep -qF "$INSTALL_DIR" "$SHELL_RC" 2>/dev/null; then
      printf '\n# zap\n%s\n' "$LINE" >> "$SHELL_RC"
      ok "Added to $SHELL_RC"
      warn "Restart your shell or run:  source $SHELL_RC"
    else
      info "$SHELL_RC already references $INSTALL_DIR"
    fi
  else
    info "Add this to your shell config:  export PATH=\"$INSTALL_DIR:\$PATH\""
  fi
fi

# ── Done ──────────────────────────────────────────────────────────────────────
printf "\n${BOLD}${GREEN}zap $VERSION installed.${RESET}\n"
printf "Run ${CYAN}zap${RESET} to start.\n\n"
