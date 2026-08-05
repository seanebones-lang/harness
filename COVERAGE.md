# Coverage Report

**Source of truth** for last measured workspace line coverage. README badge and contributor docs should match this file — do not claim the CI target as achieved coverage.

| Field | Value |
|-------|--------|
| **Tool** | cargo-llvm-cov |
| **Measured lines** | **46.52%** (10459 covered / 22481 lines) |
| **Regions** | 47.77% · **Functions** 52.52% |
| **Date** | 2026-08-05 |
| **CI target** | ≥ 60% lines on PRs (`.github/workflows/coverage.yml` via `cargo llvm-cov --fail-under-lines 60`) — **not yet met** |
| **Near-term target** | ≥ 40% workspace lines — **met** (this measure) |

Prior measures:
- llvm-cov 2026-08-03: **44.67%** lines (9723 / 21766)
- llvm-cov 2026-07-30: **40.22%** lines (7814 / 19430)
- tarpaulin 2026-05-25: **23.33%** lines (2407 / 10317)

Tooling differs across tools; treat llvm-cov as current SoT going forward.

Note: Measured % is still below the 60% PR gate. The gate is a **target**, not current status. 2026-08-05 climb: git tool ~78% lines, executor ~73%, auth_token ~85%, swarm ~87%, swarm_registry ~87%. TUI driver/input and some CLI wiring still near 0%. Climb plan: [`docs/COVERAGE_PLAN.md`](docs/COVERAGE_PLAN.md).

## How to re-run

```bash
# Preferred (matches CI)
cargo install cargo-llvm-cov   # once
cargo llvm-cov --workspace --all-features --summary-only

# Alternate (historical)
cargo tarpaulin --workspace --out Stdout --timeout 300
```

After a fresh run, update **Measured**, **Date**, and tool above. Keep the badge in `README.md` aligned with the measured figure (or label it explicitly as “target 60%”).

Do **not** invent higher numbers or imply the fail-under gate is green until a real measurement supports it.
