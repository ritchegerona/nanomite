# Installation Guide

Full platform-specific installation instructions for nanomite.

## One-liner install (recommended)

**Linux / macOS:**

```bash
curl -fsSL https://raw.githubusercontent.com/ritchegerona/nanomite/master/install.sh | bash
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/ritchegerona/nanomite/master/install.ps1 | iex
```

What the installer does:

1. Checks for Rust ≥ 1.75 (installs via rustup if missing)
2. Checks for Python ≥ 3.9 (warns if missing — needed for the cracking toolkit)
3. Installs system build dependencies (gcc, pkg-config, OpenSSL, Chromium)
4. Clones nanomite from GitHub
5. Builds the release binary (`cargo build --release`)
6. Copies the binary to `~/.local/bin` (Linux/macOS) or `%USERPROFILE%\.cargo\bin` (Windows)
7. Re-signs the binary on macOS (ad-hoc codesign, required for copied binaries to run)
8. Verifies the binary runs (`nanomite --version`)
9. Prints PATH setup instructions if the install directory is not on your PATH

## Manual build from source

### Prerequisites

| Requirement | Version | Notes |
|---|---|---|
| Rust | 1.75+ | via [rustup](https://rustup.rs) |
| Python | 3.9+ | required for the cracking toolkit |
| Build tools | — | gcc, pkg-config, OpenSSL dev headers |
| Chromium | any | required for `browse` / `browse_screenshot` tools |

### Linux

```bash
# Install build deps (Debian/Ubuntu example)
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev chromium

# Clone and build
git clone https://github.com/ritchegerona/nanomite.git
cd nanomite
cargo build --release

# Install to PATH
mkdir -p ~/.local/bin
ln -sf "$(pwd)/target/release/nanomite" ~/.local/bin/nanomite

# Add to PATH if needed
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

### macOS

```bash
# Install build deps (Homebrew)
brew install pkg-config openssl@3 chromium

# Clone and build
git clone https://github.com/ritchegerona/nanomite.git
cd nanomite
cargo build --release

# Install to PATH
mkdir -p /usr/local/bin 2>/dev/null || true
ln -sf "$(pwd)/target/release/nanomite" /usr/local/bin/nanomite

# Apple Silicon: use /opt/homebrew/bin instead
ln -sf "$(pwd)/target/release/nanomite" /opt/homebrew/bin/nanomite
```

### Windows

```powershell
# Prerequisites: Visual Studio Build Tools (MSVC), Python 3.9+
git clone https://github.com/ritchegerona/nanomite.git
cd nanomite
cargo build --release

# Add to PATH (current user)
[Environment]::SetEnvironmentVariable(
  "Path",
  [Environment]::GetEnvironmentVariable("Path", "User") + ";" + "$PWD\target\release",
  "User")
```

- Run inside **Windows Terminal** (full virtual-terminal support)
- Microsoft Edge is preinstalled and auto-discovered for browse tools
- Restart your shell after PATH changes

## Python toolkit (cracking engine)

Optional but required for HACKER MODE password-audit operations:

```bash
pip install numpy scikit-learn onnxruntime llama-cpp-python pytest mypy ruff
```

The toolkit lives under `src/nanomite/cracking/`.

## First run

```bash
cd your-project
nanomite
```

1. Type `/` and pick **/connect** (or `/provider` to browse providers)
2. Choose a provider, paste your API key — saved to `~/.config/nanomite/config.toml` with owner-only permissions
3. Pick a model with `/models` and start working

Keyless local options (no API key needed):
- Ollama: `/provider → ollama (http://localhost:11434/v1)`
- LM Studio: `/provider → lmstudio (http://localhost:1234/v1)`

## Enabling HACKER MODE

HACKER MODE is disabled by default. To enable:

1. Set `hacker_enabled = true` in `~/.config/nanomite/config.toml`
2. Set `hacker_scope` to your authorized CIDRs/domains:

```toml
[hacker]
enabled = true
scope = "10.0.0.0/8,example.com"
session_minutes = 60
store = true
audit = true
```

Or via environment variables:

```bash
export NANOMITE_HACKER=1
export NANOMITE_HACKER_SCOPE="10.0.0.0/8 example.com"
```

Then run `/hacker` to activate.

## Keeping up to date

For git-checkout installs:

```bash
nanomite update
```

Fetches origin, fast-forwards the checkout, rebuilds the release binary in
place, and reports the new version.

Or re-run the one-liner installer for the latest `master`.

## Troubleshooting

| Problem | Fix |
|---|---|
| `cargo: command not found` | Restart your shell after rustup install, or add `$HOME/.cargo/bin` to PATH |
| `nanomite: killed` on macOS | Run `codesign --force --sign - $(which nanomite)` |
| `browse` tools fail | Install Chromium/Chrome/Edge and ensure it's discoverable |
| Cracking toolkit import errors | Run `pip install numpy scikit-learn onnxruntime llama-cpp-python pytest mypy ruff` |
| `install.sh` fails on package manager | Install deps manually: gcc, pkg-config, libssl-dev, chromium |
| `install.ps1` fails on MSVC | Install Visual Studio Build Tools from https://visualstudio.microsoft.com/downloads/#build-tools-for-visual-studio-2022 |
