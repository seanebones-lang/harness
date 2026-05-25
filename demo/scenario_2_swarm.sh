#!/usr/bin/env bash
# Scenario 2: Parallel sub-agent swarm
#
# This scenario demonstrates the harness swarm subsystem. Three sub-agents run
# concurrently, each reviewing a different aspect of the harness-tools crate.
# Results are stored in the SQLite swarm registry (~/.harness/swarm.db) and can
# be retrieved after the tasks complete.
#
# Prerequisites:
#   - harness on PATH (run demo/setup.sh first)
#   - Ollama running, or a cloud API key set

set -euo pipefail

echo "=== Scenario 2: Parallel sub-agent swarm ==="
echo ""

echo "--- Queuing swarm task (3 parallel agents) ---"
harness swarm run --agents 3 "Review the harness-tools crate and summarize the three most important tools"

echo ""
echo "--- Current swarm task list ---"
harness swarm list

echo ""
echo "--- Tip: poll for completion with ---"
echo "  harness swarm list"
echo "  harness swarm status <task-id>"
echo "  harness swarm result <task-id>"
