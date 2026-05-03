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
if [[ -f "Cargo.toml" ]]; then
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

VERSION=$("$INSTALL_DIR/$BINARY" --version 2>/dev/null || echo "unknown")
info "Installed $VERSION"
info "Run: harness"
info "Or:  ANTHROPIC_API_KEY=sk-ant-... harness \"your prompt\""
