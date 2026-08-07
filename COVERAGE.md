# Coverage Report

**Source of truth** for last measured workspace line coverage. README badge and contributor docs should match this file — do not claim the CI target as achieved coverage.

| Field | Value |
|-------|--------|
| **Tool** | cargo-llvm-cov |
| **Measured lines** | **57.13%** (14212 covered / 24878 lines) |
| **Regions** | 58.60% · **Functions** 63.80% |
| **Date** | 2026-08-07 cont6 (memory_project + status/input titles) |
| **CI target** | ≥ 60% lines on PRs (`.github/workflows/coverage.yml` via `cargo llvm-cov --fail-under-lines 60`) — **not yet met** |
| **Near-term target** | ≥ 40% workspace lines — **met** (this measure) |

Prior measures:
- llvm-cov 2026-08-07 cont5 slash parsers: **56.60%** lines (14010 / 24751)
- llvm-cov 2026-08-07 cont4 notify+cost: **56.31%** lines (13877 / 24646)
- llvm-cov 2026-08-07 cont3 theme+resume: **56.18%** lines (13820 / 24600)
- llvm-cov 2026-08-07 cont2: **55.60%** · cont wiring: **55.05%** · morning: **54.51%**
- llvm-cov 2026-08-06: **53.47%** · swarm-50: **51.98%**

Tooling differs across tools; treat llvm-cov as current SoT going forward.

Note: Measured % is still below the 60% PR gate. Climb 2026-08-07 cont6: path-inject memory remember/forget/list/load + sanitize; render `input_bar_title` / `status_indicators` / `format_status_bar_line`. render ~26% lines · memory ~92%. Residual: draw_* frames, slash async I/O, build_tools_inner, tui/mod. Climb plan: [`docs/COVERAGE_PLAN.md`](docs/COVERAGE_PLAN.md).

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
