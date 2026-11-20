#!/bin/bash
# Nanomite Command Center Backend Startup Script

# Run from security-platform root so dotenvy can load .env (APP__ prefixed)
cd "$(dirname "$0")/../.."

exec cargo run --bin main --manifest-path packages/backend/Cargo.toml "$@"
