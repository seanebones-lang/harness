#!/usr/bin/env bash
# W1.2 offline Linux REL-01 subset via Docker (no API keys).
# Usage: from repo root — bash scripts/smoke_linux_docker.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

IMAGE="${HARNESS_LINUX_SMOKE_IMAGE:-rust:1.95.0-bookworm}"
info() { printf '\033[32m[linux-smoke]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[linux-smoke]\033[0m %s\n' "$*"; }
die() { printf '\033[31m[linux-smoke]\033[0m %s\n' "$*"; exit 1; }

command -v docker >/dev/null 2>&1 || die "docker not installed"

info "image=$IMAGE mount=$ROOT"
docker run --rm \
  -v "$ROOT:/src:ro" \
  -w /tmp/harness-smoke \
  -e CARGO_TERM_COLOR=always \
  "$IMAGE" \
  bash -lc '
set -euo pipefail
apt-get update -qq
apt-get install -y -qq pkg-config libssl-dev cmake clang python3 >/dev/null
# copy writable tree (source is ro)
tar -C /src --exclude=target --exclude=.git --exclude=".env*" --exclude=.harness --exclude=.obsidian --exclude=Vault --exclude="*.pem" --exclude="*.key" --exclude="*.p12" --exclude="*.keystore" --exclude="*.db*" --exclude=node_modules --exclude=apps/desktop/src-tauri/target -cf - . | tar -xf -
# drop host target artifacts that may be wrong arch
rm -rf target
cargo build --locked -q --bin harness
python3 scripts/smoke_runtime.py target/debug/harness
python3 scripts/smoke_agent.py target/debug/harness
echo LINUX_OFFLINE_SMOKE_OK
'

info "Linux offline smoke finished (see container stdout for LINUX_OFFLINE_SMOKE_OK)."
warn "Key-dependent one-shot/TUI still manual (W1.1–W1.3 full)."
