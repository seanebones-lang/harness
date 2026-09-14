#!/usr/bin/env bash
# harness install script — prefers prebuilt binaries from GitHub Releases
# Falls back to building from source if no release is available.
#
# Usage (recommended):
#   curl -fsSL https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.sh | bash
#
# Or with a specific version:
#   curl -fsSL ... | bash -s -- v1.3.0

set -euo pipefail

REPO="seanebones-lang/harness"
INSTALL_DIR="${HARNESS_INSTALL_DIR:-$HOME/.local/bin}"
BINARY="harness"

info()  { printf "\033[32m[harness]\033[0m %s\n" "$*"; }
warn()  { printf "\033[33m[harness]\033[0m %s\n" "$*"; }
error() { printf "\033[31m[harness]\033[0m %s\n" "$*" >&2; exit 1; }

# Detect target triple
detect_target() {
    local os arch
    os=$(uname -s | tr '[:upper:]' '[:lower:]')
    arch=$(uname -m)

    case "$os" in
        darwin)
            case "$arch" in
                arm64|aarch64) echo "aarch64-apple-darwin" ;;
                x86_64)        echo "x86_64-apple-darwin" ;;
                *)             error "Unsupported macOS arch: $arch" ;;
            esac
            ;;
        linux)
            case "$arch" in
                arm64|aarch64) echo "aarch64-unknown-linux-gnu" ;;
                x86_64)        echo "x86_64-unknown-linux-gnu" ;;
                *)             error "Unsupported Linux arch: $arch" ;;
            esac
            ;;
        msys*|mingw*|cygwin*)
            # Windows (Git Bash / MSYS2)
            echo "x86_64-pc-windows-msvc"
            ;;
        *)
            error "Unsupported OS: $os"
            ;;
    esac
}

TARGET=$(detect_target)
info "Detected target: $TARGET"

# Map Rust triple → release artifact name (matches .github/workflows/release.yml)
artifact_name() {
    case "$1" in
        x86_64-unknown-linux-gnu)   echo "harness-linux-x86_64" ;;
        aarch64-unknown-linux-gnu)  echo "harness-linux-aarch64" ;;
        x86_64-apple-darwin)        echo "harness-macos-x86_64" ;;
        aarch64-apple-darwin)       echo "harness-macos-aarch64" ;;
        x86_64-pc-windows-msvc)     echo "harness-windows-x86_64.exe" ;;
        *)                          echo "harness-$1" ;;
    esac
}

ARTIFACT=$(artifact_name "$TARGET")
[[ "$TARGET" != *windows* ]] || BINARY="harness.exe"

# Try to install a prebuilt binary from the latest release
install_prebuilt() {
    local version="${1:-latest}"
    local url
    local tmp

    if [[ "$version" == "latest" ]]; then
        url="https://github.com/$REPO/releases/latest/download/${ARTIFACT}"
    else
        url="https://github.com/$REPO/releases/download/${version}/${ARTIFACT}"
    fi

    tmp=$(mktemp -d)
    info "Downloading prebuilt binary ($version)..."
    if ! curl -fsSL "$url" -o "$tmp/harness"; then
        rm -rf "$tmp"
        warn "Prebuilt download unavailable; trying the requested source revision"
        return 1
    fi
    local expected actual
    if ! curl -fsSL "${url%/*}/checksums.txt" -o "$tmp/checksums.txt"; then
        rm -rf "$tmp"
        error "Missing release checksums; refusing to install an unverified binary"
    fi
    expected=$(awk -v artifact="$ARTIFACT" '$2 == artifact {print $1}' "$tmp/checksums.txt")
    if [[ ! "$expected" =~ ^[[:xdigit:]]{64}$ ]]; then
        rm -rf "$tmp"
        error "Expected exactly one SHA-256 checksum for $ARTIFACT"
    fi
    if command -v sha256sum >/dev/null; then
        actual=$(sha256sum "$tmp/harness" | awk '{print $1}')
    elif command -v shasum >/dev/null; then
        actual=$(shasum -a 256 "$tmp/harness" | awk '{print $1}')
    else
        rm -rf "$tmp"
        error "SHA-256 verification requires sha256sum or shasum"
    fi
    if [[ "$actual" != "$expected" ]]; then
        rm -rf "$tmp"
        error "Checksum mismatch for $ARTIFACT"
    fi
    mkdir -p "$INSTALL_DIR"
    install -m 755 "$tmp/harness" "$INSTALL_DIR/$BINARY"
    rm -rf "$tmp"
    info "Checksum verified; installed $INSTALL_DIR/$BINARY"
}
# Fallback: build from source
build_from_source() {
    if ! command -v cargo &>/dev/null; then
        error "cargo not found. Install Rust first: https://rustup.rs"
    fi

    info "Building from source (this may take a few minutes)..."

    local src_dir scratch="" version="${1:-latest}"
    if [[ "$version" == "latest" && -f "Cargo.toml" && -f "crates/harness-provider-core/Cargo.toml" ]]; then
        src_dir="."
    else
        scratch=$(mktemp -d)
        src_dir="$scratch/harness"
        if [[ "$version" == "latest" ]]; then
            git clone --depth=1 "https://github.com/$REPO.git" "$src_dir" || { rm -rf "$scratch"; error "Source clone failed"; }
        else
            git clone --depth=1 --branch "$version" "https://github.com/$REPO.git" "$src_dir" || { rm -rf "$scratch"; error "Requested version not found: $version"; }
        fi
    fi
    if ! (cd "$src_dir" && cargo build --locked --profile release-lto); then
        [[ -z "$scratch" ]] || rm -rf "$scratch"
        error "Source build failed"
    fi
    mkdir -p "$INSTALL_DIR"
    install -m 755 "$src_dir/target/release-lto/$BINARY" "$INSTALL_DIR/$BINARY"
    [[ -z "$scratch" ]] || rm -rf "$scratch"

    info "Built and installed from source"
}

# Main
mkdir -p "$INSTALL_DIR"

# Warn if another harness binary exists on PATH (common: ~/.cargo/bin vs ~/.local/bin)
if command -v harness >/dev/null 2>&1; then
    existing=$(command -v harness)
    planned="$INSTALL_DIR/harness"
    if [[ "$existing" != "$planned" && "$existing" != "$planned.exe" ]]; then
        warn "Another harness binary is already on PATH: $existing"
        warn "This install will place the binary at: $planned"
        warn "Ensure $INSTALL_DIR appears before other bin dirs in your PATH."
    fi
fi
if [[ -x "$HOME/.cargo/bin/harness" && "$INSTALL_DIR" != "$HOME/.cargo/bin" ]]; then
    warn "$HOME/.cargo/bin/harness also exists — cargo install and this script use different paths."
fi

if [[ "${HARNESS_INSTALL_SOURCE:-0}" == "1" ]] || ! install_prebuilt "${1:-latest}"; then
    build_from_source "${1:-latest}"
fi

# PATH warning
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
    warn "$INSTALL_DIR is not in PATH. Add this to your shell profile:"
    warn "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

# Setup owns route selection and config creation.
VERSION=$("$INSTALL_DIR/$BINARY" --version)
info "Installed $VERSION"
info "Run: harness setup"
