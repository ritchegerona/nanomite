#!/usr/bin/env bash
# nanomite — unified installer (curl | sh)
#   curl -fsSL https://raw.githubusercontent.com/ritchegerona/nanomite/master/install.sh | bash
#
# Installs: Rust toolchain, system build deps, Chromium (for browse tools), nanomite binary

set -euo pipefail

# ── Configuration ──────────────────────────────────────────────────────────
REPO="ritchegerona/nanomite"
INSTALL_DIR="${HOME}/.local/bin"
BUILD_DIR="/tmp/nanomite-build-$$"
MIN_RUST_VERSION="1.75"   # MSRV

# ── Helpers ────────────────────────────────────────────────────────────────
log()   { printf "\033[1;32m[%s]\033[0m %s\n" "$(date +%H:%M:%S)" "$*"; }
warn()  { printf "\033[1;33m[%s]\033[0m %s\n" "$(date +%H:%M:%S)" "$*"; }
error() { printf "\033[1;31m[%s]\033[0m %s\n" "$(date +%H:%M:%S)" "$*" >&2; }
die()   { error "$*"; exit 1; }

has()   { command -v "$1" >/dev/null 2>&1; }
ver_ge() { [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]; }

# ── OS detection ───────────────────────────────────────────────────────────
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$OS" in
  linux)   OS="linux" ;;
  darwin)  OS="macos" ;;
  *)       die "Unsupported OS: $OS (only Linux and macOS supported)" ;;
esac
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64)  ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *)       die "Unsupported arch: $ARCH" ;;
esac
log "Detected: $OS / $ARCH"

# ── 1. Ensure Rust toolchain ───────────────────────────────────────────────
if has cargo; then
  RUSTV="$(cargo --version | cut -d' ' -f2)"
  if ver_ge "$RUSTV" "$MIN_RUST_VERSION"; then
    log "Rust $RUSTV OK (≥ $MIN_RUST_VERSION)"
  else
    warn "Rust $RUSTV < $MIN_RUST_VERSION — updating via rustup"
    rustup update stable
  fi
else
  log "Installing Rust (rustup)..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y --no-modify-path --profile minimal
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
fi

# Make cargo available for the rest of the script
export PATH="$HOME/.cargo/bin:$PATH"

# ── 2. System build dependencies ───────────────────────────────────────────
install_linux_deps() {
  if has apt-get; then
    log "Installing build deps via apt (Debian/Ubuntu)..."
    sudo apt-get update -qq
    sudo apt-get install -y -qq \
      build-essential pkg-config libssl-dev \
      chromium-browser 2>/dev/null || sudo apt-get install -y -qq chromium
  elif has dnf; then
    log "Installing build deps via dnf (Fedora/RHEL)..."
    sudo dnf install -y -q \
      gcc gcc-c++ make pkgconfig openssl-devel \
      chromium 2>/dev/null || sudo dnf install -y -q chromium-browser
  elif has pacman; then
    log "Installing build deps via pacman (Arch)..."
    sudo pacman -Sy --needed --noconfirm \
      base-devel pkgconf openssl \
      chromium
  elif has zypper; then
    log "Installing build deps via zypper (openSUSE)..."
    sudo zypper install -y \
      gcc gcc-c++ make pkg-config libopenssl-devel \
      chromium
  elif has apk; then
    log "Installing build deps via apk (Alpine)..."
    sudo apk add --no-cache \
      build-base pkgconf openssl-dev \
      chromium
  else
    warn "Unknown package manager — skipping system deps. You may need:"
    warn "  gcc, pkg-config, libssl-dev (or openssl-devel), chromium"
  fi
}

install_macos_deps() {
  if has brew; then
    log "Installing build deps via Homebrew..."
    brew install --quiet pkg-config openssl@3 2>/dev/null || true
    # Chromium for browse tools — use chromium (open-source) or google-chrome
    if ! has chromium && ! has "Google Chrome"; then
      log "Installing Chromium for browse tools..."
      brew install --quiet --cask chromium 2>/dev/null ||
        brew install --quiet --cask google-chrome 2>/dev/null ||
        warn "Could not install Chromium — browse tools will not work"
    fi
  else
    warn "Homebrew not found — install Xcode CLT and Chromium manually:"
    warn "  xcode-select --install"
    warn "  brew install chromium  # or download Chrome"
  fi
}

case "$OS" in
  linux)  install_linux_deps ;;
  macos)  install_macos_deps ;;
esac

# ── 3. Build nanomite ──────────────────────────────────────────────────────
log "Cloning $REPO..."
rm -rf "$BUILD_DIR"
git clone --depth 1 "https://github.com/$REPO.git" "$BUILD_DIR"

log "Building release binary (this takes ~2-3 min)..."
cd "$BUILD_DIR"
cargo build --release --locked

# ── 4. Install binary ──────────────────────────────────────────────────────
mkdir -p "$INSTALL_DIR"
cp "target/release/nanomite" "$INSTALL_DIR/nanomite"
log "Installed to $INSTALL_DIR/nanomite"

# ── 5. PATH check ──────────────────────────────────────────────────────────
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
  warn "$INSTALL_DIR is not in your PATH."
  warn "Add this to your shell config (~/.bashrc, ~/.zshrc, ~/.config/fish/config.fish):"
  printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR"
fi

# ── 6. Cleanup ─────────────────────────────────────────────────────────────
rm -rf "$BUILD_DIR"

# ── 7. Verify ──────────────────────────────────────────────────────────────
if "$INSTALL_DIR/nanomite" --version >/dev/null 2>&1; then
  VER="$("$INSTALL_DIR/nanomite" --version)"
  log "✅ $VER installed successfully!"
  log "Run 'nanomite' to start. First run will guide you through /connect."
else
  die "Binary built but verification failed."
fi

# ── 8. Next steps ──────────────────────────────────────────────────────────
cat <<'EOF'

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
NEXT STEPS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

1) Start the agent:
   nanomite

2) Configure a provider (first run):
   /connect        # pick provider, paste API key
   /models         # pick a model

   Keyless local options (no API key needed):
   - Ollama:       /provider → ollama (http://localhost:11434/v1)
   - LM Studio:    /provider → lmstudio (http://localhost:1234/v1)

3) Enable browser tools (optional):
   Linux:  sudo apt install chromium      # already done by this script
   macOS:  brew install --cask chromium
   Windows: Edge is preinstalled

4) Keep updated:
   nanomite update   # git-checkout installs only
   # Or re-run this installer for the latest master

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
EOF