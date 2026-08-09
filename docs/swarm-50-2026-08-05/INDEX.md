# Swarm-50 — NextEleven Harness (2026-08-05)

**Root:** `<repo-root>`  
**Branch:** `main` @ start `b29993f`  
**Orchestrator:** Hermes + specialized `delegate_task` agents  
**Goal:** 50 focused eng iterations on open residuals (skip 📌 billing + keys-only)  
**Notes folder:** this directory · iterations in `iterations/`  
**Vault mirror:** `Vault/Swarm-50/` · HQ: `NextEleven-HQ/Projects/Harness.md`

## Open eng residuals (SoT)

| ID | Work | Status |
|----|------|--------|
| W2.1+ | Coverage climb toward 60% (now 46.52%) | in progress |
| W2.3 | Crates unwrap residual audit | open |
| W1.1 | REL-01 live keys | blocked keys |
| W1.2 | Linux Docker smoke | Docker daemon fail on host |
| W1.3 | Windows smoke | blocked env |
| W1.4–W1.5 | Billing/prebuilts | 📌 PINNED |
| W7.6 | Stable 0.2.0 | blocked smoke matrix |
| Docs | CTO exec findings drift (Gemini/Bedrock still “open”) | open |
| Hygiene | `src/swarm 2.rs`, `CLAUDE 2.md` junk | open |

## Roster

| Agent | Lanes |
|-------|-------|
| `quality-engineer` | unit tests, coverage climb, pure helpers |
| `tools-engineer` | harness-tools gaps (gh/search/selfdev/test_runner) |
| `docs-release` | CTO/TODO/RELEASE/COVERAGE honesty |
| `docs-vault` | Vault + HQ Obsidian links |
| `hygiene` | junk files, dead paths, clippy cleanliness |
| `orch` | integrate, cargo gates, commit, log |

## Rules

1. Quote path: `cd "<repo-root>"`
2. Children **do not commit**
3. One cargo test filter per invocation
4. Skip 📌 billing + live API keys
5. Log every iteration under `iterations/NNN-*.md`
6. Link from `Vault/Swarm-50/Index.md` + HQ Activity Log

## Progress

- Target: **50** iterations
- Completed: see `PROGRESS.md`
- Baseline gate: `cargo test --bin harness` → **123** pass @ start
