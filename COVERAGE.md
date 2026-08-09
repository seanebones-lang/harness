# Coverage Report

**Source of truth** for last measured workspace line coverage. README badge and contributor docs should match this file — do not claim the CI target as achieved coverage.

| Field | Value |
|-------|--------|
| **Tool** | cargo-llvm-cov |
| **Measured lines** | **61.65%** (16497 covered / 26757 lines) |
| **Regions** | 63.37% · **Functions** 68.04% |
| **Date** | 2026-08-09 Swarm-51 (residual pure-test climb) |
| **CI target** | ≥ 60% lines on PRs (`.github/workflows/coverage.yml` via `cargo llvm-cov --fail-under-lines 60`) — **met (measured)** |
| **Near-term target** | ≥ 40% workspace lines — **met** |

Prior measures:
- llvm-cov 2026-08-07 cont6: **57.13%** lines (14212 / 24878)
- llvm-cov 2026-08-07 cont5 slash parsers: **56.60%** lines (14010 / 24751)
- llvm-cov 2026-08-07 cont4 notify+cost: **56.31%** lines (13877 / 24646)
- llvm-cov 2026-08-07 cont3 theme+resume: **56.18%** lines (13820 / 24600)
- llvm-cov 2026-08-07 cont2: **55.60%** · cont wiring: **55.05%** · morning: **54.51%**
- llvm-cov 2026-08-06: **53.47%** · swarm-50: **51.98%**

Tooling differs across tools; treat llvm-cov as current SoT going forward.

Note: Swarm-51 specialized agents + parent pure edges crossed the **60%** line gate. High climbers: cost/cost_db, observability path-inject, args clap matrix, tools pure edges (tools package **179**), tui events/driver/render residual. Still low: `tui/mod`, `main` dispatch, parts of `wiring`/`driver`/`input` I/O loops. Climb plan: [`docs/COVERAGE_PLAN.md`](docs/COVERAGE_PLAN.md). Notes: [`docs/swarm-51-2026-08-09/`](docs/swarm-51-2026-08-09/).

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
