#!/usr/bin/env bash
# REL-01 automated subset — run locally before manual API-key smoke (docs/PUBLIC_RELEASE.md §3).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

HARNESS="${HARNESS_BIN:-$ROOT/target/release-lto/harness}"
if [[ ! -x "$HARNESS" ]]; then
  HARNESS="${HARNESS_BIN:-$ROOT/target/debug/harness}"
fi
if [[ ! -x "$HARNESS" ]]; then
  echo "Build harness first: cargo build --profile release-lto"
  exit 1
fi

info() { printf "\033[32m[smoke]\033[0m %s\n" "$*"; }
warn() { printf "\033[33m[smoke]\033[0m %s\n" "$*"; }

info "harness --version"
"$HARNESS" --version

info "harness doctor"
"$HARNESS" doctor || warn "doctor reported issues (keys may be missing)"

info "harness update"
"$HARNESS" update

info "harness sessions (empty ok)"
"$HARNESS" sessions

info "harness setup --help path"
"$HARNESS" setup --help >/dev/null

if curl -fsS "http://127.0.0.1:8787/api/health" >/dev/null 2>&1; then
  info "serve already up — /api/health ok"
else
  warn "harness serve not running — start manually for web UI smoke"
fi

warn "Manual steps still required (need API keys):"
warn "  - one-shot: harness \"Reply with exactly: OK\""
warn "  - TUI: harness"
warn "  - serve + browser chat"
warn "  - harness export <id>"

info "Automated REL-01 subset complete."
