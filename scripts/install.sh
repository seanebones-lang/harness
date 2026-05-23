#!/usr/bin/env bash
# harness install script — prefers prebuilt binaries from GitHub Releases
# Falls back to building from source if no release is available.
#
# Usage (recommended):
#   curl -fsSL https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.sh | bash
#
# Or with a specific version:
#   curl -fsSL ... | bash -s -- v0.2.0

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

# Try to install a prebuilt binary from the latest release
install_prebuilt() {
    local version="${1:-latest}"
    local url
    local tmp

    if [[ "$version" == "latest" ]]; then
        url="https://github.com/$REPO/releases/latest/download/harness-${TARGET}"
        if [[ "$TARGET" == *"windows"* ]]; then
            url="${url}.exe"
        fi
    else
        url="https://github.com/$REPO/releases/download/${version}/harness-${TARGET}"
        if [[ "$TARGET" == *"windows"* ]]; then
            url="${url}.exe"
        fi
    fi

    tmp=$(mktemp -d)

    info "Downloading prebuilt binary ($version)..."
    if curl -fsSL "$url" -o "$tmp/harness" 2>/dev/null || curl -fsSL "$url.exe" -o "$tmp/harness.exe" 2>/dev/null; then
        # Try to verify checksum if available
        checksum_url="${url%/*}/checksums.txt"
        if curl -fsSL "$checksum_url" -o "$tmp/checksums.txt" 2>/dev/null; then
            if command -v sha256sum >/dev/null; then
                if (cd "$tmp" && sha256sum -c checksums.txt --ignore-missing 2>/dev/null); then
                    info "Checksum verified"
                fi
            fi
        fi
        mkdir -p "$INSTALL_DIR"
        if [[ "$TARGET" == *"windows"* ]]; then
            install -m 755 "$tmp/harness.exe" "$INSTALL_DIR/harness.exe"
        else
            chmod +x "$tmp/harness"
            install -m 755 "$tmp/harness" "$INSTALL_DIR/harness"
        fi
        info "Installed prebuilt binary to $INSTALL_DIR"
        return 0
    else
        warn "No prebuilt binary found for $version (release may not exist yet)"
        return 1
    fi
}

# Fallback: build from source
build_from_source() {
    if ! command -v cargo &>/dev/null; then
        error "cargo not found. Install Rust first: https://rustup.rs"
    fi

    info "Building from source (this may take a few minutes)..."

    local src_dir
    if [[ -f "Cargo.toml" ]]; then
        src_dir="."
    else
        src_dir=$(mktemp -d)
        trap 'rm -rf "$src_dir"' EXIT
        git clone --depth=1 "https://github.com/$REPO.git" "$src_dir/harness"
        src_dir="$src_dir/harness"
    fi

    (cd "$src_dir" && cargo build --profile release-lto)

    mkdir -p "$INSTALL_DIR"
    if [[ "$TARGET" == *"windows"* ]]; then
        install -m 755 "$src_dir/target/release-lto/harness.exe" "$INSTALL_DIR/harness.exe"
    else
        install -m 755 "$src_dir/target/release-lto/harness" "$INSTALL_DIR/harness"
    fi

    info "Built and installed from source"
}

# Main
mkdir -p "$INSTALL_DIR"

if ! install_prebuilt "${1:-latest}"; then
    build_from_source
fi

# PATH warning
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
    warn "$INSTALL_DIR is not in PATH. Add this to your shell profile:"
    warn "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

# Create default config
mkdir -p "$HOME/.harness"
if [[ ! -f "$HOME/.harness/config.toml" ]]; then
    info "Creating default config at ~/.harness/config.toml"
    cat >"$HOME/.harness/config.toml" <<'EOF'
[provider]
# api_key = "sk-ant-..."   # or set ANTHROPIC_API_KEY env var
model = "claude-sonnet-4-6"
max_tokens = 8192
temperature = 0.7

[memory]
enabled = true
embed_model = "nomic-embed-text"

[agent]
system_prompt = """
You are a powerful coding assistant running in a terminal.

Available tools:
  read_file, write_file     — read or overwrite files
  patch_file                — surgical old→new text replacement (prefer this over write_file for edits)
  list_dir                  — list directory contents
  shell                     — run shell commands (build, test, git, etc.)
  search_code               — regex search across the codebase
  spawn_agent               — run a sub-agent with base tools for parallel tasks
  browser (when enabled)    — Chrome CDP: navigate, screenshot, click, fill forms
  MCP tools (when loaded)   — any tools registered via .harness/mcp.json

Guidelines:
  - Prefer patch_file over write_file for targeted edits.
  - Always run tests or build commands after changes to verify correctness.
  - Be concise. Prefer making changes over explaining them.
  - When editing multiple files, use spawn_agent for parallelism.
  - In plan mode (--plan flag), destructive calls pause for user approval.
"""
EOF
fi

VERSION=$("$INSTALL_DIR/harness" --version 2>/dev/null || echo "unknown")
info "Installed $VERSION"
info "Run: harness"
info "Update: re-run this install script or add: alias harness-update=\"curl -fsSL ... | bash\""
info "Or:  ANTHROPIC_API_KEY=sk-ant-... harness \"your prompt\""