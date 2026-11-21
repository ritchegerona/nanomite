# Installation

nanomite is a single Rust crate (`nanomite-ai`, edition 2021) that builds to a
self-contained terminal binary. No runtime services, no model server, and no
network dependency are required to run.

## Prerequisites

| Requirement | Version | Notes |
|---|---|---|
| Rust | 1.75+ | via [rustup](https://rustup.rs) |
| Build tools | — | C compiler, pkg-config, OpenSSL dev headers |
| Chromium | any | required for `browse` / `browse_screenshot` tools |
| Python | 3.9+ | optional, for the Python toolkit under `tools/custom/` |

## Build from source

```bash
git clone https://github.com/ritchegerona/nanomite.git
cd nanomite
cargo build --release
```

The release binary is written to `target/release/nanomite-ai`. Copy or symlink
it onto your `PATH`:

```bash
mkdir -p ~/.local/bin
ln -sf "$(pwd)/target/release/nanomite-ai" ~/.local/bin/nanomite
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

macOS: if you copy the binary to a new path, re-sign it ad-hoc so Gatekeeper
allows it to run:

```bash
codesign --force --sign - ~/.local/bin/nanomite
```

## Feature-gated builds

The `nanomite-vpn` binary and Tor control library are behind optional features:

```bash
cargo build --release --features vpn          # VPN only
cargo build --release --features vpn,tor      # VPN + Tor (deepweb over SOCKS5)
```

Tor itself is a separate system daemon; nanomite controls it via
`/tor start|stop|status` but does not bundle the `tor` binary. Install it via
your package manager (`apt install tor` / `brew install tor`) before use.

## First run

```bash
cd your-project
nanomite
```

1. Press `/` and choose `/connect` (or `/provider` to browse providers).
2. Choose a provider and paste your API key. It is saved to
   `~/.config/nanomite/config.toml` with owner-only permissions (`0600`) and
   is never logged.
3. Pick a model with `/models` and start working.

Keyless local options (no API key needed):

- Ollama: `http://localhost:11434/v1`
- LM Studio: `http://localhost:1234/v1`

## Keeping up to date

For git-checkout installs:

```bash
nanomite update
```

This fetches origin, fast-forwards the checkout, rebuilds the release binary in
place, and reports the new version. Alternatively re-run the installer for the
latest `master`.

## Troubleshooting

| Problem | Fix |
|---|---|
| `cargo: command not found` | Restart your shell after rustup install, or add `$HOME/.cargo/bin` to PATH |
| `nanomite: killed` on macOS | Run `codesign --force --sign - $(which nanomite)` |
| `browse` tools fail | Install Chromium/Chrome/Edge and ensure it is discoverable on PATH |
| Python toolkit import errors | Run `pip install -r tools/custom/requirements.txt` (if present) |
