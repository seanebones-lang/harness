#!/usr/bin/env bash
# Refresh every formula checksum only after verifying the complete Unix artifact set.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REPO="seanebones-lang/harness"
FORMULA="${HARNESS_HOMEBREW_FORMULA:-$ROOT/homebrew/harness.rb}"
VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | python3 -c 'import json,sys; print(json.load(sys.stdin)["tag_name"])')
fi
[[ "$VERSION" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || { echo "Invalid release tag" >&2; exit 1; }
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT
base="https://github.com/$REPO/releases/download/$VERSION"
curl -fsSL "$base/checksums.txt" -o "$tmpdir/checksums.txt"
for artifact in harness-macos-aarch64 harness-macos-x86_64 harness-linux-aarch64 harness-linux-x86_64; do
  echo "[homebrew] Verifying $artifact" >&2
  curl -fsSL "$base/$artifact" -o "$tmpdir/$artifact"
done
python3 - "$FORMULA" "$VERSION" "$tmpdir" <<'PY'
import hashlib
from pathlib import Path
import re
import sys
formula, version, downloads = Path(sys.argv[1]), sys.argv[2], Path(sys.argv[3])
manifest = (downloads / 'checksums.txt').read_text().splitlines()
text = formula.read_text()
for artifact in ['harness-macos-aarch64', 'harness-macos-x86_64', 'harness-linux-aarch64', 'harness-linux-x86_64']:
    matches = [line.split()[0] for line in manifest if len(line.split()) == 2 and line.split()[1] == artifact]
    actual = hashlib.sha256((downloads / artifact).read_bytes()).hexdigest()
    if matches != [actual]:
        raise SystemExit(f'Checksum missing, duplicated, or mismatched: {artifact}')
    pattern = r'(url "[^"\n]*/' + re.escape(artifact) + r'"\s*\n\s*sha256 ")[^"]+(")'
    # Match the existing URL/checksum pair regardless of whether it is a placeholder.
    text, count = re.subn(pattern, lambda m: m[1] + actual + m[2], text)
    if count != 1:
        raise SystemExit(f'Expected one formula entry: {artifact}')
text, count = re.subn(r'(?m)^  version "[^"]+"$', f'  version "{version[1:]}"', text)
if count != 1:
    raise SystemExit('Expected one formula version')
# No formula mutation happens before all artifacts and substitutions validate.
formula.write_text(text)
print(f'Updated {formula} for {version}')
PY
