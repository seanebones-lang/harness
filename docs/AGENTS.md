# Specialized Agents — harness docs + eng

**Orchestrator:** Hermes  
**Project:** `/Users/nexteleven/Desktop/harness rework` (or clone root)  
**Backlog:** `docs/CTO_BACKLOG.md`  
**Branch:** **`main`**  
**Concurrency:** max 3 implementers

## Roster

| Agent ID | Specialty | Touches |
|----------|-----------|---------|
| `docs-readme` | Full product README honesty | `README.md` |
| `docs-dev` | Developer map | `CLAUDE.md`, `ARCHITECTURE.md` |
| `docs-user` | Cookbook / shortcuts / install | `docs/COOKBOOK.md`, `SHORTCUTS.md`, `INSTALL.md` |
| `docs-release` | Release + backlog truth | `TODO.md`, `CTO_BACKLOG.md`, `RELEASE_STATUS.md`, `TEAM_UPDATE_*` |
| `provider-engineer` | Providers | `crates/harness-provider-*` |
| `tools-engineer` | Optional tools | `crates/harness-tools`, wiring, config |
| `quality-engineer` | Tests / coverage | `src/*` tests, `COVERAGE.md` |
| `pr-shipper` | Commit/push on `main` | git/gh |

## Rules

1. Non-overlapping paths in parallel.
2. Docs: no MIT claims; license = proprietary NextEleven LLC.
3. Coverage badge = measured SoT in `COVERAGE.md`.
4. CLI flags: verify with `./target/debug/harness`, not PATH.
5. Children do not commit; orchestrator ships to **`main`**.
6. Entry: `pwd && ls -la && git status` (quote paths with spaces).

## Docs wave (2026-08-03)

- readme · claude · todo · shortcuts · cto header · team update — **done**
