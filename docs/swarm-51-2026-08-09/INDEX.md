# Swarm-51 — NextEleven Harness (2026-08-09)

**Root:** `<repo-root>`  
**Branch:** `main` @ start `cd9c5bd`  
**Orchestrator:** Hermes + specialized `delegate_task` agents  
**Durable swarm id:** `swe184ad90`  
**Goal:** 50 focused eng iterations — residual coverage climb toward **60%** (baseline **57.13%**)  
**Notes folder:** this directory · iterations in `iterations/`  
**Vault mirror:** `Vault/Swarm-51/` · HQ: `NextEleven-HQ/Projects/Harness.md` · Activity Log

## Open eng residuals (SoT)

| ID | Work | Status |
|----|------|--------|
| W2.1+ | Coverage climb 57.13% → 60% | **in progress** |
| W2.3 | Crates unwrap residual | open/low |
| W1.1 | REL-01 live keys | blocked keys |
| W1.2 | Linux Docker smoke | Docker daemon |
| W1.3 | Windows smoke | blocked env |
| W1.4–W1.5 | Billing/prebuilts | 📌 PINNED |
| W7.6 | Stable 0.2.0 | blocked smoke matrix |

## Roster

| Agent | Owns (disjoint) |
|-------|-----------------|
| `quality-tui` | `src/tui/{driver,mod,events,confirm_flow}.rs` pure tests |
| `quality-cli` | `src/cli/{wiring,lightweight,args}.rs` pure helpers/tests |
| `quality-server` | `src/server/{collab_ws,mod,state}.rs` pure tests |
| `quality-core` | `src/{checkpoint,ambient,cost_db,sync,daemon,bridges}.rs` pure edges |
| `tools-engineer` | `crates/harness-tools` residual pure edges only |
| `docs-release` | CTO/TODO/RELEASE/COVERAGE/Vault honesty after remeasure |
| `orch` | hygiene junk, integrate races, gates, commit, HQ log |

## Rules

1. Quote path: `cd "<repo-root>"`
2. Children **do not commit**
3. One cargo test filter per invocation
4. Skip 📌 billing + live API keys
5. Log every iteration under `iterations/NNN-lane.md`
6. Link from `Vault/Swarm-51/Index.md` + HQ Activity Log
7. No parent∥child same-file `mod tests` append race
8. Prefer pure helpers wired into production (avoid dead_code under `-D warnings`)

## Progress

- Target: **50** iterations
- Completed: see `PROGRESS.md`
- Baseline tip: `cd9c5bd` · cov **57.13%** · HQ bin note **264**
