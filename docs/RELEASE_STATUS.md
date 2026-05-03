# Release status — harness

This file records the latest **go / no-go** assessment for sharing the repo publicly. Update it when you run [`PUBLIC_RELEASE.md`](PUBLIC_RELEASE.md).

## Verification log (this workspace)

**2026-05-03 — Post-push verification sweep (`main` @ `3cffa5a`)**

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | Pass |
| `cargo test --all` (incl. doctests) | Pass — **91 tests** summed across crates (prior log row cited 90 before recount) |
| `cargo build --profile release-lto` | Pass (~60s local dev machine; distro thin-LTO slice) |

| Notes | **CI parity:** `.github/workflows/ci.yml` still runs **`cargo audit`** / **`cargo deny`**, MSRV (**1.76** `cargo check --workspace --all-targets`), **`cargo build --all-targets`**, and **`cargo test --all`** on **ubuntu / macos / windows** — run those locally when mirroring CI. |

---

**2026-05-03 — Phase-2 continuation: remaining CLI handlers extracted from `main.rs`**

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | Pass |
| `cargo test --all` | Pass — **91 tests** (workspace total incl. doctest stanzas; prior notes used 89→90 progression) |

| Change | Detail |
|--------|--------|
| `src/main.rs` | **823 LOC** (was ~1,539 after first project extraction; ~716 LOC moved to `cli/commands/` this round) |
| New modules | `cli/commands/{prompt,sessions,init,status,models,doctor,self_dev}.rs` — `sessions` / `export` / `delete` / `init` / `status` / `models` / `doctor` / `self-dev` (+ shared `build_prompt_with_image`) |
| Early `Project` path | Still returns before provider setup (unchanged); `match` retains `Project` arm for exhaustiveness |

**Still manual before calling it "stable":** interactive TUI on each OS you care about; `gh auth login` + `/pr` where you use GitHub; confirm `harness serve` in browser after a clean install.

---

**2026-05-03 — Phase-2 god-file split + MCP concurrency-test slice (earlier same day)**

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | Pass |
| `cargo test --all` (incl. doctests) | Pass — **89 tests** (running total before the CLI-handler continuation above) |
| `cargo build --profile release-lto` | Pass |
| New tests (that slice) | **+8** MCP in-process; **+4** TUI render; **+9** TUI events; **+7** project-command helpers |
| God-file decomposition | **`src/main.rs`** 2,203 → **1,539 LOC**; **`src/tui/mod.rs`** 2,789 → **2,065 LOC**; `src/tui/{render,events}.rs`, `src/cli/commands/project.rs`; MCP `from_streams` refactor |
| CI config | [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) — supply-chain, MSRV, multi-OS matrix, [`coverage.yml`](../.github/workflows/coverage.yml), `deny.toml` |
| Release | [`.github/workflows/release.yml`](../.github/workflows/release.yml) — version/help smokes, cross pinned `0.2.5` |

**`3fa6d51` audit remediation closed (now also verified by tests):** OpenAI multi-tool SSE flush (regression-tested in `crates/harness-provider-openai`), **MCP dedicated stdout reader (regression-tested in `crates/harness-mcp`)**, MCP sampling paths tested, `WorkspaceRoot` jail boundary-tested, `src/cli/commands/project.rs` + `src/tui/{render,events}.rs` extracted, LSP framing hardened.

**Next iteration (Phase 2 residuals):** `src/main.rs` stays **823 LOC** — optional further splits (e.g. cost/swarm match arms, `run_once` wrappers); **`src/tui`** now includes **`state` / `input` / `slash` / `render` / `events` / `driver`** (**`mod.rs` ~152 LOC**); coverage ≥60%; proptest/fuzz; `#![deny(missing_docs)]` on public crates.

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
