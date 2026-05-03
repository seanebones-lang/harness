# Release status — harness

This file records the latest **go / no-go** assessment for sharing the repo publicly. Update it when you run [`PUBLIC_RELEASE.md`](PUBLIC_RELEASE.md).

## Verification log (this workspace)

**2026-05-03 — Cross-platform beta hardening pass**

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | Pass |
| `cargo test --all` (incl. doctests) | Pass |
| `cargo build --profile release-lto` | Pass |
| Code | Windows `shell` tool: prefer `sh.exe`/`bash.exe` on `PATH`; `cmd.exe` fallback; timeout smoke test uses PowerShell on Windows |
| CI config | [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) — full test matrix + **`install-scripts`** (`scripts/install.sh` on Ubuntu + macOS, `install.ps1` on Windows) |
| Release | [`.github/workflows/release.yml`](../.github/workflows/release.yml) — per-target **`--version` / `--help`** smokes where the host can execute; **`file`** check for cross-built Linux aarch64; **`cross` pinned** (`0.2.5`) |

**Still manual before calling it “stable”:** interactive TUI on each OS you care about; `gh auth login` + `/pr` where you use GitHub; confirm `harness serve` in browser after a clean install.

---

## Current recommendation (public beta)

| Item | Status |
|------|--------|
| **License** | MIT (`LICENSE` + workspace `Cargo.toml`) |
| **Automated gates** | Local + CI: `fmt`, `clippy --all-features`, `test --all`, `build`; `release-lto` locally / release workflow for tags |
| **Docs / onboarding** | README: macOS/Linux + **Windows PowerShell**, install scripts (`install.sh`, `install.ps1`), optional-feature matrix |
| **Interactive TUI** | Confirm per platform |
| **`gh` integration** | Optional; all platforms |

**Verdict:** **GO** for **public beta** — Windows is CI-gated at the same bar as macOS/Linux; optional features remain OS-dependent (see README **Optional features by platform**). Promote to “stable” only after broader real-world use and manual checks above.

---

_Update this file when you tag a release or change licensing._
