# Specialized Agents — harness CTO backlog

**Orchestrator:** Hermes (this session)  
**Project:** `/Users/nexteleven/Desktop/harness rework`  
**Backlog:** `docs/CTO_BACKLOG.md`  
**Branch:** `dev`  
**Concurrency:** max 3 implementers (Hermes delegation cap)

## Roster

| Agent ID | Wave tasks | Specialty | Touches |
|----------|------------|-----------|---------|
| `truth-reconciler` | W0.1 | Docs honesty / TODO sync | TODO.md, CTO_BACKLOG checkboxes |
| `coverage-auditor` | W0.2 | Metrics / badge truth | README, CLAUDE, COVERAGE.md, CI badge |
| `slash-impl` | W0.3 | TUI slash → real features | src/tui/input.rs, bridges, observability |
| `vault-curator` | W0.4 | Obsidian structure | Vault/** (local only) |
| `cookbook-writer` | W0.5 | User docs examples | docs/COOKBOOK.md |
| `pr-shipper` | W0.6 | GitHub PR | gh, git |
| `release-smoker` | W1.* | REL-01 / artifacts | scripts, RELEASE_STATUS |
| `quality-engineer` | W2.* | Tests, coverage, unwraps | tests/, src/, crates/ |
| `tui-polisher` | W3.* | TUI UX + swarm ops | src/tui/, src/swarm.rs |
| `interop-engineer` | W4.* | MCP/LSP/bridges | crates/harness-mcp, bridges |
| `provider-engineer` | W5.* | New providers/tools | crates/harness-provider-* |
| `surface-packer` | W6.* | VS Code / Tauri | apps/, extensions/ |

## Rules
1. One agent owns its file set; no two implementers edit the same path in parallel.
2. Implement → self-verify (build/test where code) → report paths + commands run.
3. Orchestrator integrates, runs full `cargo test --bin harness`, commits, pushes.
4. Do not commit secrets. Do not force-push `main`.

## Wave 0 batch (NOW)
- coverage-auditor · slash-impl · cookbook-writer in parallel  
- truth-reconciler + vault-curator + pr-shipper on orchestrator after/during
