# Release status — harness

This file records the latest **go / no-go** assessment for sharing the repo publicly. Update it when you run [`PUBLIC_RELEASE.md`](PUBLIC_RELEASE.md).

## Verification log (this workspace)

**2026-05-24 — Public beta promotion report**

| Item | Result |
|------|--------|
| Promotion assessment | [`PROMOTION_REPORT.md`](PROMOTION_REPORT.md) — **GO** for public beta |
| Tier 0 (docs, CI, screenshots) | **Complete** |
| Tier 1 (REL-01, Homebrew, tag) | **Pending** — maintainer-only |
| Draft release notes | [`RELEASE_NOTES_v0.1.2-beta.md`](RELEASE_NOTES_v0.1.2-beta.md) |
| Comparison table refresh | [`COMPARISON.md`](COMPARISON.md) updated |

**Go / no-go:** **GO** to promote public beta now. **Stable** still blocked on REL-01 manual smoke §3 per OS + P2-10 Homebrew post-tag.

---

**2026-05-24 — MIT Re-Inspection Round 2 (post-`433065d`, local working tree)**

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | **Pass** |
| `cargo clippy --all-targets --all-features -- -D warnings` | **Pass** |
| `cargo test --all` | **Pass** — **218 tests** (running total across crates + doctests) |
| `cargo build --profile release-lto` | **Pass** (~65s local) |
| `scripts/smoke_rel01.sh` | **Pass** — automated REL-01 subset (doctor, update, sessions, setup --help) |
| P0 security | **Closed** — re-verified; no new P0 |
| Manual smoke §3 | **Pending** — maintainer-only (API keys + TUI + serve + export) |

| Round 2 fixes | GitHub Projects GraphQL stdin + JSON-safe query; Notes `escape_applescript`; `apply_patch` parser `Err` paths; `/api/health` `config_path` loopback-gated; `ProviderRouter::default_provider` no panic; mutex poison fail-closed (rate_limit, swarm); collab/voice/mlx/lsp tests; CI `smoke-rel01` job; release checksum hardening; Windows prebuilt in `install.ps1` |

**Go / no-go:** **GO** for public beta. **Stable** blocked on **REL-01** manual smoke §3 per target OS. Homebrew tap SHA update remains post-tag maintainer action (P2-10).

---

**2026-05-22 — Peer review remediation (security + tests + docs)**

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | Not re-run this session (prior pass) |
| `cargo clippy --all-targets --all-features -- -D warnings` | **Pass** |
| `cargo test --all` | **Pass** — **164 tests** (agent, swarm, tools, providers) |
| P0 security (tar-slip, auth, confirm gate, sandbox) | **Closed** — see [`PEER_REVIEW_AUDIT.md`](PEER_REVIEW_AUDIT.md) |
| Threat model | [`docs/THREAT_MODEL.md`](THREAT_MODEL.md) |
| Manual smoke §3 | **Pending** — checklist in [`PUBLIC_RELEASE.md`](PUBLIC_RELEASE.md) §3 (needs API keys) |
| `cargo build --profile release-lto` | **Pass** — 2026-05-22 (post security follow-up) |

**Go / no-go:** **GO** for public beta. **Stable** blocked on manual smoke §3 per target OS.

---

**2026-05-22 — TODO/CONTRIBUTING backlog implementation**

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --all-targets --all-features -- -D warnings` | Pass |
| `cargo test --all` (incl. doctests) | Pass — **114 tests** |
| `cargo build --profile release-lto` | Not re-run this session (prior May 2026 pass still valid) |
| Coverage CI | [`.github/workflows/coverage.yml`](.github/workflows/coverage.yml) — PR gate **≥ 60%** line coverage |
| Manual smoke §3 | **Pending** — needs API keys (one-shot, TUI, serve, export, sessions) |

| Delivered | Browser tests + `Err` semantics; `AmbientConfig` + consolidation tests; session title fixes; PowerShell shell; daemon TCP (Windows) + VS Code TCP; Tauri `serve` autospawn; `missing_docs` on core/tools; proptest; docs (`BROWSER_CDP`, `COOKBOOK`, `i18n/es`) |

**Go / no-go:** **GO** for continued public beta; promote to stable after manual smoke §3 on your target platforms.

---

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

**Next iteration:** manual release smoke §3; optional tools (`DatabaseTool`, `NotebookTool`, `DockerTool`); Tauri Windows/Linux packaging; CDP doc screenshots; full i18n of user manual.

---

## Current recommendation (public beta)

| Item | Status |
|------|--------|
| **License** | MIT (`LICENSE` + workspace `Cargo.toml`) |
| **Automated gates** | **218 tests**, clippy clean; CI multi-OS + `smoke-rel01` job; coverage ≥ 60% on PRs |
| **P0 security** | **Closed** — see [`PEER_REVIEW_AUDIT.md`](PEER_REVIEW_AUDIT.md) |
| **Threat model** | [`docs/THREAT_MODEL.md`](THREAT_MODEL.md) |
| **Open backlog** | [`TODO.md`](../TODO.md) — severity-ranked + roadmap |
| **Manual smoke §3** | **Pending** — blocks **stable** |
| **Experimental modules** | `collab`, `bridges`, `diff_review` wired when enabled; polish ongoing |

**Verdict:** **GO** for **public beta**. Promote to **stable** only after **REL-01** manual smoke §3 on target OSes (record above).

---

_Update this file when you tag a release or change licensing._
