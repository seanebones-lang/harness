# Release status — harness

This file records the latest **go / no-go** assessment for sharing the repo publicly. Update it when you run [`PUBLIC_RELEASE.md`](PUBLIC_RELEASE.md).

## Verification log (this workspace)

Last run: **2026-05-03** — `cargo fmt --check`, `clippy -D warnings`, `cargo test`, `cargo build --profile release-lto` all passed; `harness doctor`, one-shot prompt, `harness serve` + `/api/health`, `harness sessions`, and `harness export` exercised successfully. **Interactive TUI** and **`gh` CLI** not run in this automation pass — confirm locally before a tagged release.

---

## Current recommendation (development)

| Item | Status |
|------|--------|
| **License** | MIT (`LICENSE` + workspace `Cargo.toml`) |
| **Automated gates** | Green: `fmt`, `clippy -D warnings`, `test`, `release-lto` |
| **One-shot / serve / sessions / export** | Verified on maintainer machine where applicable |
| **Interactive TUI** | Confirm locally before wider announcement |
| **`gh` integration** | Optional; requires `gh auth login` on the user machine |

**Verdict:** **GO** to publish the source for others to build under **Beta** expectations (see README). Complete interactive TUI + GitHub CLI checks on a real workstation before calling it “stable.”

---

_Update this file when you tag a release or change licensing._
