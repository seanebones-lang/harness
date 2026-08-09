# Swarm-51 Plan — residual → 60%

## Strategy

1. Scaffold notes + vault + HQ (this folder).
2. Wave pure unit tests on residual near-0% / low-% surfaces:
   - TUI: driver/mod/events/confirm pure extracts
   - CLI: wiring pure helpers (tool registration predicates, path clamps)
   - Server: collab_ws event map + state pure
   - Core: checkpoint/ambient/cost/sync/daemon edges
   - Tools: any remaining pure validation gaps
3. Batches of ≤4 specialized leaves; orch integrates + gates.
4. Every micro-slice → `iterations/NNN-*.md`.
5. Close: `cargo test --bin harness` + clippy `-D warnings` + llvm-cov; move COVERAGE.md + badge + RELEASE_STATUS together.
6. Obsidian: Vault/Swarm-51 + Index + HQ Harness + Activity Log.

## High-ROI targets (start)

| Lane | Files | Idea |
|------|-------|------|
| tui-driver | `src/tui/driver.rs` | extract pure key/event classify if needed; unit edges |
| tui-events | `src/tui/events.rs` | AgentEvent → line kind / format pure |
| cli-wiring | `src/cli/wiring.rs` | tool enable predicates, allowlist pure |
| collab | `src/server/collab_ws.rs` | `agent_event_to_collab` pure map |
| core | checkpoint/ambient/cost_db | path-inject + disabled config |

## Gates

| Gate | Baseline |
|------|----------|
| llvm-cov lines | **57.13%** (14212/24878) |
| `cargo test --bin harness` | remeasure at start of integrate |
| clippy bin `-D warnings` | clean |
| smoke_rel01 offline | optional mid-campaign |
