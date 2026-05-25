#!/usr/bin/env bash
# Harness demo setup script
# Sets up Ollama with a local model and verifies the harness binary is available.
# No API keys required for the Ollama-backed scenarios.

set -euo pipefail

OLLAMA_MODEL="${OLLAMA_MODEL:-llama3.2:3b}"   # smaller model for fast demo; set OLLAMA_MODEL=qwen3-coder:30b for full model
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

# ── Helpers ──────────────────────────────────────────────────────────────────

ok()   { printf '\033[32m  ✓\033[0m %s\n' "$*"; }
warn() { printf '\033[33m  !\033[0m %s\n' "$*"; }
fail() { printf '\033[31m  ✗\033[0m %s\n' "$*" >&2; exit 1; }
step() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }

# ── 1. Check Docker and docker compose ───────────────────────────────────────

step "Checking Docker"

if ! command -v docker &>/dev/null; then
    fail "Docker is not installed. Install from https://docs.docker.com/get-docker/ and re-run."
fi
ok "docker found: $(docker --version)"

# Support both 'docker compose' (v2 plugin) and 'docker-compose' (v1 standalone)
if docker compose version &>/dev/null 2>&1; then
    COMPOSE="docker compose"
    ok "docker compose (v2) found"
elif command -v docker-compose &>/dev/null; then
    COMPOSE="docker-compose"
    ok "docker-compose (v1) found: $(docker-compose --version)"
else
    warn "docker compose not found — compose-based scenarios will not work, but demo scripts that use harness directly will still run."
    COMPOSE=""
fi

# ── 2. Pull Ollama model ──────────────────────────────────────────────────────

step "Setting up Ollama"

if command -v ollama &>/dev/null; then
    ok "ollama found: $(ollama --version 2>/dev/null || echo 'version unknown')"
    printf "  Pulling %s (this may take a few minutes)...\n" "$OLLAMA_MODEL"
    if ollama pull "$OLLAMA_MODEL"; then
        ok "Model $OLLAMA_MODEL is ready"
    else
        warn "ollama pull failed — demo scenarios that use Ollama will not work."
        warn "Start Ollama and run: ollama pull $OLLAMA_MODEL"
    fi
else
    warn "ollama not found. Install from https://ollama.com and run: ollama pull $OLLAMA_MODEL"
    warn "Alternatively, set ANTHROPIC_API_KEY / XAI_API_KEY / OPENAI_API_KEY before running the demo scenarios."
fi

# ── 3. Verify or build harness binary ────────────────────────────────────────

step "Checking harness binary"

if command -v harness &>/dev/null; then
    ok "harness found on PATH: $(command -v harness)"
else
    warn "harness not found on PATH — attempting to build from source"

    if ! command -v cargo &>/dev/null; then
        fail "cargo not found. Install Rust from https://rustup.rs and re-run."
    fi

    printf "  Building harness (release-lto)...\n"
    (cd "$REPO_ROOT" && cargo build --profile release-lto 2>&1)

    BUILT_BIN="$REPO_ROOT/target/release-lto/harness"
    if [[ -x "$BUILT_BIN" ]]; then
        ok "Built: $BUILT_BIN"
        warn "Add the binary to your PATH or install it:"
        warn "  install -m 755 $BUILT_BIN ~/.local/bin/harness"
        warn "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        # Temporarily wire it for the remainder of this script
        export PATH="$REPO_ROOT/target/release-lto:$PATH"
    else
        fail "Build failed. Check the output above."
    fi
fi

# ── 4. Summary ────────────────────────────────────────────────────────────────

printf '\n\033[1;32m=== Setup complete ===\033[0m\n\n'
printf 'Next steps:\n'
printf '  Run the demo scenarios in this directory:\n\n'
printf '    bash demo/scenario_1_file_analysis.sh\n'
printf '    bash demo/scenario_2_swarm.sh\n'
printf '    bash demo/scenario_3_memory.sh\n\n'
printf '  For cloud providers:\n'
printf '    export ANTHROPIC_API_KEY="sk-ant-..."\n'
printf '    harness "explain the agent loop in src/agent.rs"\n\n'
printf '  Model used for local Ollama scenarios: %s\n' "$OLLAMA_MODEL"
printf '  To use a larger model: OLLAMA_MODEL=qwen3-coder:30b bash demo/setup.sh\n\n'
