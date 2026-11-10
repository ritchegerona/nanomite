#!/usr/bin/env bash
# Install all forensic and metadata analysis tools
# Run with: sudo ./install_forensic_tools.sh

set -euo pipefail

log() { printf '\033[1;32m[%s]\033[0m %s\n' "$(date +%H:%M:%S)" "$*"; }
warn() { printf '\033[1;33m[%s]\033[0m %s\n' "$(date +%H:%M:%S)" "$*"; }
error() { printf '\033[1;31m[%s]\033[0m %s\n' "$(date +%H:%M:%S)" "$*" >&2; }
die() { error "$*"; exit 1; }

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$OS" in
  linux)   OS="linux" ;;
  darwin)  OS="macos" ;;
  *)       die "Unsupported OS: $OS" ;;
esac

if [[ "$OS" == "linux" ]]; then
  if command -v apt-get >/dev/null; then
    UPDATE="apt-get update -qq"
    INSTALL="apt-get install -y -qq"
  elif command -v dnf >/dev/null; then
    UPDATE="dnf check-update || true"
    INSTALL="dnf install -y -q"
  elif command -v pacman >/dev/null; then
    UPDATE="pacman -Sy"
    INSTALL="pacman -S --needed --noconfirm"
  elif command -v zypper >/dev/null; then
    UPDATE="zypper refresh"
    INSTALL="zypper install -y"
  else
    die "No supported package manager found"
  fi
elif [[ "$OS" == "macos" ]]; then
  if command -v brew >/dev/null; then
    UPDATE="brew update"
    INSTALL="brew install"
  else
    die "Homebrew required on macOS"
  fi
fi

log "Updating package lists..."
eval "$UPDATE"

TOOLS_LINUX=(
  "exiftool" "jhead" "imagemagick" "graphicsmagick" "binwalk" "foremost"
  "scalpel" "steghide" "stegsnow" "outguess" "zsteg" "jpegdump"
  "jpeginfo" "qpdf" "pdf-parser" "pdfid" "poppler-utils" "pdftk"
  "xxd" "sleuthkit" "autopsy" "testdisk" "hashdeep" "ssdeep"
  "tesseract-ocr" "ffmpeg" "mediainfo" "exiv2" "mat2"
  "python3-magic" "libimage-exiftool-perl"
)

TOOLS_MACOS=(
  "exiftool" "jhead" "imagemagick" "graphicsmagick" "binwalk" "foremost"
  "scalpel" "steghide" "stegsnow" "outguess" "zsteg" "jpeg"
  "jpeginfo" "qpdf" "pdf-parser" "pdfid" "poppler" "pdftk-java"
  "xxd" "sleuthkit" "autopsy" "testdisk" "hashdeep" "ssdeep"
  "tesseract" "ffmpeg" "mediainfo" "exiv2" "mat2"
)

if [[ "$OS" == "linux" ]]; then
  log "Installing Linux forensic tools..."
  eval "$INSTALL ${TOOLS_LINUX[*]}" || warn "Some tools may have failed to install"
  
  if ! command -v zsteg >/dev/null && command -v gem >/dev/null; then
    log "Installing zsteg via Ruby gem..."
    gem install zsteg || warn "zsteg gem install failed"
  fi
  
  if ! command -v pdf-parser >/dev/null; then
    log "Installing pdf-parser, pdfid via pip..."
    pip3 install --break-system-packages pdf-parser pdfid || warn "pip install failed"
  fi
  
elif [[ "$OS" == "macos" ]]; then
  log "Installing macOS forensic tools..."
  eval "$INSTALL ${TOOLS_MACOS[*]}" || warn "Some tools may have failed to install"
  
  if command -v gem >/dev/null; then
    gem install zsteg || warn "zsteg gem install failed"
  fi
  
  pip3 install pdf-parser pdfid || warn "pip install failed"
fi

log "Installing Python forensic packages..."
pip3 install --break-system-packages \
  exifread pillow numpy scikit-learn onnxruntime pytest mypy ruff \
  python-magic olefile pypdf2 pdfplumber openpyxl python-docx python-pptx \
  || warn "Some Python packages failed"

log "Forensic tools installation complete!"
log "Verify with: /home/ritche/nanomite/forensic-venv/bin/python /home/ritche/nanomite/forensic_toolkit.py --list"
