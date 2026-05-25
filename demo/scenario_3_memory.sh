#!/usr/bin/env bash
# Scenario 3: Persistent memory across sessions
#
# This scenario demonstrates harness project memory. Facts stored with
# `harness memorize` are injected into every subsequent session via the
# .harness/memory/ files, so the agent always has project context without
# the user needing to re-explain the codebase.
#
# Run the steps in order: each step builds on the previous one.
#
# Prerequisites:
#   - harness on PATH (run demo/setup.sh first)
#   - Any provider configured (Ollama or cloud API key)

set -euo pipefail

echo "=== Scenario 3: Persistent memory across sessions ==="
echo ""

# ── Step 1: Store some project facts ─────────────────────────────────────────

echo "--- Step 1: Storing project memory ---"

harness memorize architecture \
    "Monorepo: 14-crate Cargo workspace. Provider abstraction in harness-provider-core. Agent loop in src/agent.rs."

harness memorize testing \
    "Run: cargo test --all. Coverage gate: 60%. No API keys required. 218 tests on Ubuntu + macOS + Windows."

harness memorize providers \
    "Supported: Anthropic Claude 4.x, xAI Grok 4.x, OpenAI GPT-5.x, Ollama (local), MLX (Apple Silicon). Router auto-selects based on env keys."

echo ""
echo "Stored memory topics:"
harness memories

# ── Step 2: Start a new session — memories are injected automatically ─────────

echo ""
echo "--- Step 2: New session — agent recalls stored facts ---"
echo "(The agent will use the stored architecture and testing facts without being told)"
echo ""

harness "What test command should I run, and what coverage threshold does this project require?"

# ── Step 3: Show memory recall in action ──────────────────────────────────────

echo ""
echo "--- Step 3: Cross-session recall ---"

harness "How many LLM providers does this project support, and how does it choose between them?"

# ── Cleanup (optional) ────────────────────────────────────────────────────────

echo ""
echo "--- Memory topics after scenario ---"
harness memories
echo ""
echo "To clean up: harness forget architecture && harness forget testing && harness forget providers"
