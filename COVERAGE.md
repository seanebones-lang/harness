# Coverage Report

**Source of truth** for last measured workspace line coverage. README badge and contributor docs should match this file — do not claim the CI target as achieved coverage.

| Field | Value |
|-------|--------|
| **Tool** | cargo-llvm-cov |
| **Measured lines** | **40.22%** (7814 covered / 19430 lines) |
| **Regions** | 40.82% · **Functions** 46.62% |
| **Date** | 2026-07-30 |
| **CI target** | ≥ 60% lines on PRs (`.github/workflows/coverage.yml` via `cargo llvm-cov --fail-under-lines 60`) — **not yet met** |
| **Near-term target** | ≥ 40% workspace lines — **met** (this measure) |

Prior measure (tarpaulin 2026-05-25): **23.33%** lines (2407 / 10317). Tooling differs; treat llvm-cov as current SoT going forward.

Note: Measured % is still below the 60% PR gate. The gate is a **target**, not current status. Critical crates are uneven — e.g. `harness-tools` policy/shell/swarm_tool high; TUI driver/input and some CLI wiring still near 0%. Climb plan: [`docs/COVERAGE_PLAN.md`](docs/COVERAGE_PLAN.md).

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
