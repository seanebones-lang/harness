#!/usr/bin/env bash
# Offline release smoke. An explicit HARNESS_BIN always wins.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
HARNESS="${HARNESS_BIN:-$ROOT/target/release-lto/harness}"
if [[ -z "${HARNESS_BIN:-}" && ! -x "$HARNESS" ]]; then
  HARNESS="$ROOT/target/debug/harness"
fi
[[ -x "$HARNESS" ]] || { echo "Build harness or set HARNESS_BIN to an executable" >&2; exit 1; }
exec python3 "$ROOT/scripts/smoke_runtime.py" "$HARNESS"
