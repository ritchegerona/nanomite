#!/usr/bin/env bash
# nanomite — unified installer (curl | sh)
#   curl -fsSL https://raw.githubusercontent.com/ritchegerona/nanomite/master/install.sh | bash
#
# Installs: Rust toolchain, system build deps, Chromium (for browse tools),
# Python toolkit deps (cracking engine), nanomite binary.
#
set -euo pipefail

# ── Configuration ──────────────────────────────────────────────────────────
REPO="ritchegerona/nanomite"
INSTALL_DIR="${HOME}/.local/bin"
BUILD_DIR="/tmp/nanomite-build-$$"
MIN_RUST_VERSION="1.75"   # MSRV
MIN_PYTHON_VERSION="3.9"

# ── Helpers ────────────────────────────────────────────────────────────────
log()   { printf '\033[1;32m[%s]\033[0m %s\n' "$(date +%H:%M:%S)" "$*"; }
warn()  { printf '\033[1;33m[%s]\033[0m %s\n' "$(date +%H:%M:%S)" "$*"; }
error() { printf '\033[1;31m[%s]\033[0m %s\n' "$(date +%H:%M:%S)" "$*" >&2; }
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

# ── 0. Python 3.9+ check (needed for cracking toolkit) ─────────────────────
if has python3; then
  PYVER="$(python3 -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")')"
  if ver_ge "$PYVER" "$MIN_PYTHON_VERSION"; then
    log "Python $PYVER OK (≥ $MIN_PYTHON_VERSION)"
  else
    warn "Python $PYVER < $MIN_PYTHON_VERSION — cracking toolkit may not work"
  fi
else
  warn "python3 not found — cracking toolkit will not work until Python ≥ $MIN_PYTHON_VERSION is installed"
fi

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
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | \
    sh -s -- -y --no-modify-path --profile minimal
fi

# Make cargo available for the rest of the script (rustup --no-modify-path)
export PATH="$HOME/.cargo/bin:$PATH"
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

# Re-verify cargo is on PATH after sourcing
if ! has cargo; then
  die "cargo not found after rustup install. Restart your shell and re-run, or add $HOME/.cargo/bin to PATH."
fi

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
    if ! has chromium && ! has "Google Chrome"; then
      log "Installing Chromium for browse tools..."
      brew install --quiet --cask chromium 2>/dev/null || \
        brew install --quiet --cask google-chrome 2>/dev/null || \
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

# ── 3. Python toolkit dependencies (cracking engine) ───────────────────────
install_python_deps() {
  if ! has python3; then
    warn "python3 not found — skipping Python toolkit dependencies"
    return 0
  fi
  log "Installing Python toolkit dependencies (numpy, scikit-learn, onnxruntime, llama-cpp-python, pytest, mypy, ruff)..."
  python3 -m pip install --upgrade pip 2>/dev/null || warn "pip upgrade failed (continuing)"
  python3 -m pip install \
    numpy \
    scikit-learn \
    onnxruntime \
    llama-cpp-python \
    pytest \
    mypy \
    ruff 2>/dev/null || warn "Some Python packages failed to install — install manually with: pip install numpy scikit-learn onnxruntime llama-cpp-python pytest mypy ruff"
}

install_python_deps

# ── 4. Build nanomite ──────────────────────────────────────────────────────
log "Cloning $REPO..."
rm -rf "$BUILD_DIR"
git clone --depth 1 "https://github.com/$REPO.git" "$BUILD_DIR"

log "Building release binary (this takes ~2-3 min)..."
cd "$BUILD_DIR"
cargo build --release

# ── 5. Install binary ──────────────────────────────────────────────────────
mkdir -p "$INSTALL_DIR"
cp "target/release/nanomite" "$INSTALL_DIR/nanomite"

# macOS kills a copied binary whose only signature is the linker's ad-hoc one
# ("zsh: killed", SIGKILL) once it lives at a new path. Re-sign it ad-hoc so the
# installed copy actually runs.
if [[ "$(uname -s)" == "Darwin" ]] && command -v codesign >/dev/null 2>&1; then
  codesign --force --sign - "$INSTALL_DIR/nanomite" >/dev/null 2>&1 || \
    warn "codesign failed — if 'nanomite' is killed on launch, run: codesign --force --sign - $INSTALL_DIR/nanomite"
fi
log "Installed to $INSTALL_DIR/nanomite"

# ── 6. PATH check ──────────────────────────────────────────────────────────
if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
  warn "$INSTALL_DIR is not in your PATH."
  warn "Add this to your shell config (~/.bashrc, ~/.zshrc, ~/.config/fish/config.fish):"
  printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR"
fi

# ── 7. Cleanup ─────────────────────────────────────────────────────────────
rm -rf "$BUILD_DIR"

# ── 8. Verify ──────────────────────────────────────────────────────────────
if "$INSTALL_DIR/nanomite" --version >/dev/null 2>&1; then
  VER="$("$INSTALL_DIR/nanomite" --version)"
  log "✅ $VER installed successfully!"
  log "Run 'nanomite' to start. First run will guide you through /connect."
else
  die "Binary built but verification failed."
fi

# ── 9. Next steps ──────────────────────────────────────────────────────────
cat <<'EOF'

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
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

4) Python cracking toolkit (optional, for HACKER MODE password audits):
   pip install numpy scikit-learn onnxruntime llama-cpp-python pytest mypy ruff

5) Keep updated:
   nanomite update   # git-checkout installs only
   # Or re-run this installer for the latest master

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
EOF
