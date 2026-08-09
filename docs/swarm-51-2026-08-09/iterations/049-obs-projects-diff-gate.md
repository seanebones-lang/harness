# 049 · gate · obs + projects + diff_review

**Time:** 2026-08-09 swarm-51  
**Child:** no commit

## Commands
```bash
cd "/Users/nexteleven/Desktop/harness rework"
cargo test --bin harness observability
cargo test --bin harness projects
cargo test --bin harness diff_review
```

## Results (this lane)
| Filter | Result |
|--------|--------|
| `observability` | **15 passed** (was 1) |
| `projects` | **17 passed** (15 projects + 2 bridges github_projects*) |
| `diff_review` | **36 passed** (was 22) |

## Touched
- `src/observability.rs`
- `src/projects.rs`
- `src/diff_review.rs`
- notes `040-obs-*` … `049-*-gate.md`

## Constraints honored
- Path-inject + tempfile; no CWD-only asserts
- One `mod tests` per file
- Avoided parent lanes: cost_db/daemon/sync/bridges/tui/events
