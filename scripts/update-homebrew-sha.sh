#!/usr/bin/env bash
# Refresh SHA256 placeholders in homebrew/harness.rb from the latest GitHub Release.
set -euo pipefail

REPO="seanebones-lang/harness"
FORMULA="homebrew/harness.rb"
VERSION="${1:-}"

if [[ -z "$VERSION" ]]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" | grep -m1 '"tag_name"' | sed -E 's/.*"tag_name": "([^"]+)".*/\1/')
fi

info() { printf "\033[32m[homebrew]\033[0m %s\n" "$*"; }

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

fetch_sha() {
  local artifact="$1"
  local url="https://github.com/$REPO/releases/download/${VERSION}/${artifact}"
  info "Downloading $artifact …"
  curl -fsSL "$url" -o "$tmpdir/$artifact"
  shasum -a 256 "$tmpdir/$artifact" | awk '{print $1}'
}

MAC_ARM=$(fetch_sha "harness-macos-aarch64")
MAC_X64=$(fetch_sha "harness-macos-x86_64")
LINUX_ARM=$(fetch_sha "harness-linux-aarch64")
LINUX_X64=$(fetch_sha "harness-linux-x86_64")

info "Updating $FORMULA for $VERSION"
sed -i.bak \
  -e "s/version \".*\"/version \"${VERSION#v}\"/" \
  "$FORMULA"

# Replace SHA lines in order: mac arm, mac x64, linux arm, linux x64
awk -v v="$VERSION" -v a="$MAC_ARM" -v b="$MAC_X64" -v c="$LINUX_ARM" -v d="$LINUX_X64" '
  /on_macos/ { mac=1; linux=0 }
  /on_linux/ { linux=1; mac=0 }
  mac && /sha256/ && !done_arm { sub(/REPLACE_WITH_ACTUAL_SHA/, a); done_arm=1; next }
  mac && /sha256/ && done_arm && !done_x64 { sub(/REPLACE_WITH_ACTUAL_SHA/, b); done_x64=1; next }
  linux && /sha256/ && !larm { sub(/REPLACE_WITH_ACTUAL_SHA/, c); larm=1; next }
  linux && /sha256/ && larm && !lx64 { sub(/REPLACE_WITH_ACTUAL_SHA/, d); lx64=1; next }
  { print }
' "$FORMULA" > "$FORMULA.tmp" && mv "$FORMULA.tmp" "$FORMULA"

rm -f "$FORMULA.bak"
info "Done. Review git diff and commit homebrew/harness.rb"
