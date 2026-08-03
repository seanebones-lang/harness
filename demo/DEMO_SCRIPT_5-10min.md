# 5-10 Minute Competition Demo Script Outline (W8.5)

**Goal**: Showcase offline-first, Docker, TUI, swarm, doctor in <10min for judges.

## Script (≈7 min)

1. **0:00-1:00** Doctor + offline setup
   - `docker compose up` (Ollama + harness, no keys)
   - `harness doctor` — verify local Ollama, no cloud keys, bridges

2. **1:00-3:00** One-shot + TUI basic
   - One-shot: `harness "analyze this repo structure"`
   - Launch TUI: `harness` (interactive)
   - Show /help, file search, shell tool

3. **3:00-5:30** Swarm parallel agents
   - `harness swarm list`
   - Spawn tasks: multiple sub-agents on file analysis / tests
   - `harness swarm status <id>` ; TUI F2 swarm panel
   - `harness swarm gc --dry-run`

4. **5:30-7:00** Advanced + export
   - `/trace` observability
   - Session export to MD
   - Confirm 100% offline, reproducible

5. **7:00-8:00** Q&A buffer + close
   - `cargo test --all` quick (offline)
   - Link to SUBMISSION_MANIFEST.md + TECHNICAL_REPORT.md

**Commands pre-cached in demo/scenario_* scripts for smooth run.**
**No API keys; pure local Ollama.**
