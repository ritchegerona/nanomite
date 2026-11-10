#!/usr/bin/env bash
# End-to-end smoke test: drive the REAL Nanomite provider stack (not curl)
# against a live OpenAI-compatible endpoint, and assert a token comes back.
#
# This exists because curl-testing an endpoint only proves the endpoint lives.
# It does NOT prove Nanomite's own request builder, SSE parser and error
# mapping work against that gateway. This does.
#
# Usage:
#   scripts/smoke-provider.sh                 # openrouter-free (uses saved key)
#   scripts/smoke-provider.sh opencode        # any preset id
#   scripts/smoke-provider.sh openrouter-free nvidia/nemotron-3.5-lightning:free
#
# Exit 0 when a real completion came back through Nanomite.
set -uo pipefail

PRESET="${1:-openrouter-free}"
MODEL="${2:-}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 1

pass=0; fail=0
ok()   { printf '  \033[32mPASS\033[0m %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  \033[31mFAIL\033[0m %s\n' "$1"; fail=$((fail+1)); }
info() { printf '  \033[90m----\033[0m %s\n' "$1"; }
hdr()  { printf '\n\033[1m%s\033[0m\n' "$1"; }

hdr "0. preset present"
if grep -q "id: \"$PRESET\"" src/config.rs; then
  ok "preset \"$PRESET\" is declared"
else
  bad "preset \"$PRESET\" NOT found in src/config.rs"
  exit 1
fi

# The key must be visible to the binary. Prefer the env var the preset names;
# fall back to the key saved in the live config for that provider.
KEY_VAR=""
case "$PRESET" in
  openrouter*) KEY_VAR="OPENROUTER_API_KEY" ;;
  opencode*)   KEY_VAR="OPENCODE_API_KEY" ;;
esac

if [ -n "$KEY_VAR" ] && [ -z "${!KEY_VAR:-}" ]; then
  saved=$(sed -n "s/^${PRESET%%-free}[[:space:]]*=[[:space:]]*\"\(.*\)\"/\1/p" \
    "$HOME/.config/nanomite/config.toml" 2>/dev/null | head -1)
  if [ -n "$saved" ]; then
    export "$KEY_VAR=$saved"
    ok "$KEY_VAR loaded from saved config (len ${#saved})"
  else
    info "$KEY_VAR unset and nothing saved for $PRESET"
  fi
else
  ok "$KEY_VAR already in environment"
fi

hdr "1. build a tiny harness that exercises the real client"
# A standalone bin cannot reach into the crate's private modules, so we drive
# the public path instead: NANOMITE_* env + the crate's own public API.
mkdir -p /tmp/smoke_provider/src
cat > /tmp/smoke_provider/Cargo.toml <<TOML
[package]
name = "smoke_provider"
version = "0.0.0"
edition = "2021"

[dependencies]
nanomite = { path = "$ROOT" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
futures-util = { version = "0.3", default-features = false, features = ["std"] }

[[bin]]
name = "smoke_provider"
path = "src/main.rs"
TOML

cat > /tmp/smoke_provider/src/main.rs <<'RS'
use nanomite::agent::provider::{AgentProvider, ChatMessage};

#[tokio::main]
async fn main() -> Result<(), String> {
    let cfg = nanomite::config::Config::from_env();
    let provider = nanomite::agent::openai::OpenAiProvider::new(&cfg)
        .map_err(|e| format!("client build failed: {e}"))?;

    let models = provider.list_models().await.map_err(|e| e.to_string())?;
    eprintln!("MODELS_OK {}", models.len());

    let events = provider
        .stream_chat(
            vec![ChatMessage::user("Reply with exactly one word: pong")],
            None,
        )
        .await
        .map_err(|e| format!("stream failed: {e}"))?;

    use futures_util::StreamExt;
    let mut stream = events;
    let mut text = String::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(nanomite::agent::provider::StreamEvent::Delta(d)) => text.push_str(&d),
            Ok(_) => {}
            Err(e) => return Err(format!("stream error: {e}")),
        }
        if text.to_lowercase().contains("pong") {
            break;
        }
    }
    if text.trim().is_empty() {
        return Err("stream produced no answer text".into());
    }
    println!("ANSWER: {}", text.trim());
    Ok(())
}
RS

info "harness written to /tmp/smoke_provider"

hdr "2. run it through Nanomite's own client"
export NANOMITE_PROVIDER="$PRESET"
[ -n "$MODEL" ] && export NANOMITE_MODEL="$MODEL"
out=$(cd "$ROOT" && cargo run --quiet --manifest-path /tmp/smoke_provider/Cargo.toml 2>&1)
code=$?

models=$(printf '%s' "$out" | grep -o 'MODELS_OK [0-9]*' | head -1)
answer=$(printf '%s' "$out" | grep -o 'ANSWER: .*' | head -1)

if [ -n "$models" ]; then
  ok "list_models via Nanomite: $models"
else
  info "list_models did not report (endpoint may not expose /models to this key)"
fi

if [ "$code" -eq 0 ] && [ -n "$answer" ]; then
  ok "stream_chat via Nanomite: $answer"
else
  bad "stream_chat failed (exit $code)"
  printf '%s\n' "$out" | tail -20 | sed 's/^/      /'
fi

hdr "summary"
printf '  preset=%s%s  %d passed, %d failed\n' "$PRESET" \
  "${MODEL:+ model=$MODEL}" "$pass" "$fail"
[ "$fail" -eq 0 ] || exit 1
