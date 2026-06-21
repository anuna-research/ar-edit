#!/bin/bash
set -e

# ar-edit Installer
# Usage: curl -fsSL https://files.anuna.io/ar-edit/latest/install.sh | bash

TOOL_NAME="ar-edit"
VERSION="${VERSION:-latest}"
BASE_URL="https://files.anuna.io/ar-edit"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

detect_platform() {
  local detected_os detected_arch
  detected_os="$(uname -s 2>/dev/null | tr '[:upper:]' '[:lower:]')"
  detected_arch="$(uname -m 2>/dev/null)"

  case "$detected_os" in
    linux*)  OS="linux" ;;
    darwin*) OS="macos" ;;
    *)       error "Unsupported OS: $detected_os. For Windows, download from: $BASE_URL/latest/ar-edit-windows-x86_64.zip" ;;
  esac

  case "$detected_arch" in
    x86_64|amd64)  ARCH="x86_64" ;;
    arm64|aarch64) ARCH="arm64" ;;
    *)             error "Unsupported architecture: $detected_arch" ;;
  esac

  PLATFORM="${OS}-${ARCH}"
  info "Detected platform: $PLATFORM"
}

get_version() {
  if [ "$VERSION" = "latest" ]; then
    info "Fetching latest version..."
    VERSION=$(curl -fsSL "$BASE_URL/latest/version.json" 2>/dev/null \
      | grep -o '"version"[[:space:]]*:[[:space:]]*"[^"]*"' | cut -d'"' -f4)
    if [ -z "$VERSION" ]; then
      error "Could not determine latest version"
    fi
    info "Latest version: $VERSION"
  fi
}

install_binary() {
  info "Installing $TOOL_NAME v$VERSION for $PLATFORM..."

  ARCHIVE_NAME="ar-edit-${PLATFORM}.tar.gz"
  DOWNLOAD_URL="$BASE_URL/v$VERSION/$ARCHIVE_NAME"

  TMP_DIR=$(mktemp -d)
  trap 'rm -rf "$TMP_DIR"' EXIT

  info "Downloading from $DOWNLOAD_URL..."
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$DOWNLOAD_URL" -o "$TMP_DIR/ar-edit.tar.gz" || error "Download failed"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$DOWNLOAD_URL" -O "$TMP_DIR/ar-edit.tar.gz" || error "Download failed"
  else
    error "curl or wget is required"
  fi

  tar -xzf "$TMP_DIR/ar-edit.tar.gz" -C "$TMP_DIR"

  mkdir -p "$INSTALL_DIR"
  mv "$TMP_DIR/ar-edit" "$INSTALL_DIR/ar-edit"
  chmod +x "$INSTALL_DIR/ar-edit"

  info "Installed to $INSTALL_DIR/ar-edit"

  if echo "$PATH" | tr ':' '\n' | grep -q "^${INSTALL_DIR}$"; then
    : # already in PATH
  else
    warn "$INSTALL_DIR is not in your PATH"
    echo "  Add it: export PATH=\"$INSTALL_DIR:\$PATH\""
  fi
}

main() {
  echo "================================"
  echo "  ar-edit Installer"
  echo "================================"
  echo ""

  detect_platform
  get_version
  install_binary

  echo ""
  info "Installation complete!"
  echo ""
  echo "Quick start:"
  echo "  ar-edit --help"
  echo ""
  echo "Documentation: https://codeberg.org/anuna/ar-edit"
}

main "$@"
