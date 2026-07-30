# Coverage Report

**Source of truth** for last measured workspace line coverage. README badge and contributor docs should match this file — do not claim the CI target as achieved coverage.

| Field | Value |
|-------|--------|
| **Tool** | cargo-tarpaulin |
| **Measured** | **23.33%** line coverage (2407 / 10317 lines) |
| **Date** | 2026-05-25 |
| **CI target** | ≥ 60% lines on PRs (`.github/workflows/coverage.yml` via `cargo llvm-cov --fail-under-lines 60`) — **not yet met** |

Note: Measured % is well below the 60% PR gate. The gate is a **target**, not current status. Acceptable for beta; uplift is tracked (CTO W2.1 / IMPROVEMENTS_TODO). HTML report, if generated, may appear under `coverage/` (gitignored or local-only).

## How to re-run

Prefer a quick summary (minutes, not hours). Full instrumented runs can be slow on large workspaces.

```bash
# Preferred (matches CI tool)
cargo install cargo-llvm-cov   # once
cargo llvm-cov --workspace --all-features --summary-only

# Alternate (matches this file’s historical tool)
cargo install cargo-tarpaulin  # once
cargo tarpaulin --workspace --out Stdout --timeout 300
```

After a fresh run, update **Measured**, **Date**, and tool above. Keep the badge in `README.md` aligned with the measured figure (or label it explicitly as “target 60%” if you drop the measured badge).

Do **not** invent higher numbers or imply the fail-under gate is green until a real measurement supports it.
