#!/usr/bin/env bash
# harness install script — builds from source and installs to ~/.local/bin
# Usage: curl -fsSL https://raw.githubusercontent.com/seanebones-lang/harness/main/scripts/install.sh | bash
set -euo pipefail

REPO_URL="https://github.com/seanebones-lang/harness"
INSTALL_DIR="${HARNESS_INSTALL_DIR:-$HOME/.local/bin}"
BINARY="harness"

info()  { printf "\033[32m[harness]\033[0m %s\n" "$*"; }
warn()  { printf "\033[33m[harness]\033[0m %s\n" "$*"; }
error() { printf "\033[31m[harness]\033[0m %s\n" "$*" >&2; exit 1; }

# Check for Rust/cargo
if ! command -v cargo &>/dev/null; then
    error "cargo not found. Install Rust first: https://rustup.rs"
fi

RUST_VERSION=$(rustc --version | awk '{print $2}')
info "Rust $RUST_VERSION detected"

# Ensure install dir exists
mkdir -p "$INSTALL_DIR"

# Clone or use existing source
if [[ -d "Cargo.toml" ]] || [[ -f "Cargo.toml" ]]; then
    info "Building from current directory"
    SRC_DIR="."
else
    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT
    info "Cloning $REPO_URL..."
    git clone --depth=1 "$REPO_URL" "$TMP_DIR/harness"
    SRC_DIR="$TMP_DIR/harness"
fi

info "Building release binary..."
(cd "$SRC_DIR" && cargo build --release --profile release-lto 2>&1)

BIN_PATH="$SRC_DIR/target/release-lto/$BINARY"
if [[ ! -f "$BIN_PATH" ]]; then
    # Fall back to regular release if LTO profile not used
    BIN_PATH="$SRC_DIR/target/release/$BINARY"
    (cd "$SRC_DIR" && cargo build --release 2>&1)
fi

info "Installing $BINARY to $INSTALL_DIR..."
install -m 755 "$BIN_PATH" "$INSTALL_DIR/$BINARY"

# Check PATH
if ! echo "$PATH" | grep -q "$INSTALL_DIR"; then
    warn "$INSTALL_DIR is not in PATH. Add this to your shell profile:"
    warn "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

# Create default config dir
mkdir -p "$HOME/.harness"
if [[ ! -f "$HOME/.harness/config.toml" ]]; then
    info "Creating default config at ~/.harness/config.toml"
    cat >"$HOME/.harness/config.toml" <<'EOF'
[provider]
# api_key = "xai-..."      # or set XAI_API_KEY env var
model = "grok-3-fast"
max_tokens = 8192
temperature = 0.7

[memory]
enabled = true
embed_model = "grok-3-embed-english"

[agent]
system_prompt = """
You are a powerful coding assistant running in a terminal.
You have access to tools to read and write files, run shell commands, and search code.
You can also spawn sub-agents for parallel tasks using the spawn_agent tool.
Be concise and precise. Prefer making changes over explaining.
"""
EOF
fi

VERSION=$("$INSTALL_DIR/$BINARY" --version 2>/dev/null || echo "unknown")
info "Installed $VERSION"
info "Run: harness"
info "Or:  XAI_API_KEY=xai-... harness \"your prompt\""
