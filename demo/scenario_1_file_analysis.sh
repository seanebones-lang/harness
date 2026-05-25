#!/usr/bin/env bash
# Scenario 1: File analysis — no API key required (uses Ollama)
#
# This scenario demonstrates harness one-shot mode: a single prompt is sent
# to the agent, it reads the repository structure using the list_dir and
# read_file tools, and prints a summary to stdout.
#
# Prerequisites:
#   - harness on PATH (run demo/setup.sh first)
#   - Ollama running with llama3.2:3b or qwen3-coder:30b pulled
#
# To use a cloud provider instead:
#   ANTHROPIC_API_KEY=sk-ant-... bash demo/scenario_1_file_analysis.sh

set -euo pipefail

echo "=== Scenario 1: Repository file analysis ==="
echo "Provider: Ollama (local, no API key required)"
echo "Model: ${OLLAMA_MODEL:-llama3.2:3b}"
echo ""

harness "List the top-level directories in this repo and explain what each one does in one sentence"
