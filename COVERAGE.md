# Coverage Report

**Source of truth** for last measured workspace line coverage. README badge and contributor docs should match this file — do not claim the CI target as achieved coverage.

| Field | Value |
|-------|--------|
| **Tool** | cargo-llvm-cov |
| **Measured lines** | **55.60%** (13603 covered / 24468 lines) |
| **Regions** | 57.03% · **Functions** 62.40% |
| **Date** | 2026-08-07 cont2 (slash + lightweight + input pure) |
| **CI target** | ≥ 60% lines on PRs (`.github/workflows/coverage.yml` via `cargo llvm-cov --fail-under-lines 60`) — **not yet met** |
| **Near-term target** | ≥ 40% workspace lines — **met** (this measure) |

Prior measures:
- llvm-cov 2026-08-07 cont wiring+render: **55.05%** lines (13405 / 24350)
- llvm-cov 2026-08-07 morning confirm/collab/driver: **54.51%** lines (13180 / 24179)
- llvm-cov 2026-08-06 project_ops+AppState: **53.47%** lines (12770 / 23884)
- llvm-cov 2026-08-05 swarm-50: **51.98%** lines (12224 / 23516)
- llvm-cov 2026-08-05 earlier: **46.52%** lines (10459 / 22481)
- llvm-cov 2026-08-03: **44.67%** lines (9723 / 21766)
- llvm-cov 2026-07-30: **40.22%** lines (7814 / 19430)
- tarpaulin 2026-05-25: **23.33%** lines (2407 / 10317)

Tooling differs across tools; treat llvm-cov as current SoT going forward.

Note: Measured % is still below the 60% PR gate. Climb 2026-08-07 cont2: `tui/slash` @file expand/complete + detect (~94% lines); `cli/lightweight` MCP allowlist + path resolve (~12%); `tui/input` search nav + trust hint pure. Residual: slash-command async body, draw_* frames, build_tools_inner. Climb plan: [`docs/COVERAGE_PLAN.md`](docs/COVERAGE_PLAN.md).

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
