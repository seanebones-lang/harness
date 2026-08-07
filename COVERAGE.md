# Coverage Report

**Source of truth** for last measured workspace line coverage. README badge and contributor docs should match this file — do not claim the CI target as achieved coverage.

| Field | Value |
|-------|--------|
| **Tool** | cargo-llvm-cov |
| **Measured lines** | **56.18%** (13820 covered / 24600 lines) |
| **Regions** | 57.55% · **Functions** 62.74% |
| **Date** | 2026-08-07 cont3 (theme + resume pure) |
| **CI target** | ≥ 60% lines on PRs (`.github/workflows/coverage.yml` via `cargo llvm-cov --fail-under-lines 60`) — **not yet met** |
| **Near-term target** | ≥ 40% workspace lines — **met** (this measure) |

Prior measures:
- llvm-cov 2026-08-07 cont2 slash/lightweight/input: **55.60%** lines (13603 / 24468)
- llvm-cov 2026-08-07 cont wiring+render: **55.05%** lines (13405 / 24350)
- llvm-cov 2026-08-07 morning confirm/collab/driver: **54.51%** lines (13180 / 24179)
- llvm-cov 2026-08-06 project_ops+AppState: **53.47%** lines (12770 / 23884)
- llvm-cov 2026-08-05 swarm-50: **51.98%** lines (12224 / 23516)
- llvm-cov 2026-08-03: **44.67%** lines (9723 / 21766)
- llvm-cov 2026-07-30: **40.22%** lines (7814 / 19430)

Tooling differs across tools; treat llvm-cov as current SoT going forward.

Note: Measured % is still below the 60% PR gate. Climb 2026-08-07 cont3: `tui/theme` parse_color + load_from_str/path + assistant_label (~98% lines); `tui/resume` full role formatting + turn counts (~99%). Residual: draw_* frames, handle_slash_command async, build_tools_inner, tui/mod. Climb plan: [`docs/COVERAGE_PLAN.md`](docs/COVERAGE_PLAN.md).

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
