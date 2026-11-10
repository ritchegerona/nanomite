#!/usr/bin/env bash
# Smoke test for the Nanomite "OpenCode" provider preset.
#
# Checks, in order:
#   1. the preset is present in the shipped roster, with the right base URL
#   2. the endpoint is reachable and serves a model catalog
#   3. free-tier ids are still refused for non-OpenCode clients (the known wall)
#   4. a paid id accepts the key (the path this preset actually uses)
#
# Usage:
#   scripts/smoke-opencode.sh              # catalog + free-wall checks only
#   OPENCODE_API_KEY=sk-... scripts/smoke-opencode.sh   # also does a live chat
#
# Exit status: 0 if every check that could run passed, 1 otherwise.
set -uo pipefail

BASE="https://opencode.ai/inference/openai/v1"
MODELS="https://opencode.ai/inference/v1/models"
DEFAULT_MODEL="gpt-5.4-nano"
pass=0; fail=0
ok()   { printf '  \033[32mPASS\033[0m %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  \033[31mFAIL\033[0m %s\n' "$1"; fail=$((fail+1)); }
info() { printf '  \033[90m----\033[0m %s\n' "$1"; }
hdr()  { printf '\n\033[1m%s\033[0m\n' "$1"; }

hdr "1. preset present in roster"
if grep -q 'id: "opencode"' src/config.rs 2>/dev/null; then
  ok 'preset id "opencode" declared in src/config.rs'
else
  bad 'preset id "opencode" NOT found in src/config.rs (run from the repo root)'
fi
if grep -q 'label: "OpenCode"' src/config.rs 2>/dev/null; then
  ok 'label is "OpenCode"'
else
  bad 'label "OpenCode" NOT found'
fi
if grep -qF "$BASE" src/config.rs 2>/dev/null; then
  ok "base URL matches $BASE"
else
  bad "base URL $BASE NOT found in src/config.rs"
fi

hdr "2. endpoint reachability + catalog"
code=$(curl -sS --max-time 25 -o /tmp/smoke_oc_models.json -w '%{http_code}' "$MODELS" 2>/dev/null)
if [ "$code" = "200" ]; then
  n=$(grep -o '"id":"[^"]*"' /tmp/smoke_oc_models.json | wc -l)
  ok "GET $MODELS -> HTTP 200 ($n models)"
  if grep -q "\"$DEFAULT_MODEL\"" /tmp/smoke_oc_models.json; then
    ok "default model \"$DEFAULT_MODEL\" is in the served catalog"
  else
    info "default model \"$DEFAULT_MODEL\" not seen in catalog (may still be routable)"
  fi
else
  bad "GET $MODELS -> HTTP $code (expected 200)"
fi

hdr "3. free-tier wall (expected to still refuse us)"
for m in pulse-alpha-free crouton-alpha-free big-pickle; do
  body=$(curl -sS --max-time 20 -X POST "$BASE/chat/completions" \
    -H 'content-type: application/json' \
    -d "{\"model\":\"$m\",\"messages\":[{\"role\":\"user\",\"content\":\"ping\"}],\"max_tokens\":5}" 2>/dev/null)
  if printf '%s' "$body" | grep -q 'FreeUsageLimitError'; then
    ok "$m -> FreeUsageLimitError (wall intact, as documented)"
  elif printf '%s' "$body" | grep -q '"choices"'; then
    info "$m -> SUCCEEDED; the wall has lifted - update the preset comment"
    pass=$((pass+1))
  else
    info "$m -> unexpected: $(printf '%s' "$body" | head -c 160)"
  fi
done

hdr "4. live chat on the paid path"
if [ -n "${OPENCODE_API_KEY:-}" ]; then
  code=$(curl -sS --max-time 30 -o /tmp/smoke_oc_chat.json -w '%{http_code}' \
    -X POST "$BASE/chat/completions" \
    -H 'content-type: application/json' \
    -H "authorization: Bearer $OPENCODE_API_KEY" \
    -d "{\"model\":\"$DEFAULT_MODEL\",\"messages\":[{\"role\":\"user\",\"content\":\"reply with the single word: pong\"}],\"max_tokens\":10}" 2>/dev/null)
  if [ "$code" = "200" ] && grep -q '"choices"' /tmp/smoke_oc_chat.json; then
    txt=$(python3 -c 'import json,sys;print(json.load(open("/tmp/smoke_oc_chat.json"))["choices"][0]["message"]["content"])' 2>/dev/null)
    ok "authenticated chat on $DEFAULT_MODEL -> HTTP 200, reply: ${txt:-<empty>}"
  else
    bad "authenticated chat -> HTTP $code: $(head -c 200 /tmp/smoke_oc_chat.json)"
  fi
else
  info "OPENCODE_API_KEY unset - skipping live chat (set it to exercise this)"
fi

hdr "summary"
printf '  %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ] || exit 1
