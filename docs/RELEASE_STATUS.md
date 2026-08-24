# Release status — NextEleven Harness

This file records the latest **go / no-go** assessment for sharing the repo publicly. Update it when you run [`PUBLIC_RELEASE.md`](PUBLIC_RELEASE.md).

## Verification log (this workspace)

**2026-08-24 — Provider-neutral routing and extensibility release gate**

| Item | Result |
|------|--------|
| Branch | **`main`** |
| Runtime policy | Exact user-selected primary + ordered fallbacks; no provider/model ranking and no implicit Ollama insertion |
| Setup / CLI | `harness setup`; `route show/set/model/add/remove/move/custom`; active/global/project scope |
| Provider surface | 18 alphabetical built-in names + custom OpenAI-format HTTP(S) endpoints; model always user-selected |
| Secret posture | Setup and dashboard store credential environment-variable names, not credential values; compatibility load for existing `api_key` remains |
| `cargo test --bin harness` | **376 passed** |
| `cargo test -p harness-provider-router` | **15 passed** |
| `cargo test --workspace --all-features` | **passed** |
| MSRV audit / `cargo +1.95.0 check --workspace --all-targets` | **passed**; locked dependencies require Rust 1.94.1+, so the stale 1.76 claim and CI job were corrected to 1.95 |
| `cargo fmt --all -- --check` | **clean** |
| `cargo clippy --all-targets --all-features -- -D warnings` | **clean** |
| Route CLI smoke | **passed** — exact set/move/show plus authenticated and unauthenticated custom endpoint config |
| Browser setup JavaScript | **parsed cleanly** with `node --check` |
| `scripts/smoke_rel01.sh` | **automated subset passed**; live provider/TUI/browser checks remain separate manual gates |
| Windows CLI subprocess regression | First remote run exposed the default PE stack limit after the CLI expansion; follow-up reserves 8 MiB for debug/integration reliability, then replaced slow serial optional-process probes in `doctor` with bounded loopback/PATH checks |

**Go / no-go:** local release gate **GO**. Remote CI result is recorded in the GitHub Actions run for the release commit. Stable remains blocked on the separate REL-01 smoke matrix and release-artifact billing gates.

---

**2026-08-09 — v1.3.0 public POC cut (proprietary)**

| Item | Result |
|------|--------|
| Branch | **`main`** · visibility **PUBLIC** (POC only) |
| Version | **1.3.0** workspace (was 0.1.2-beta) — Cargo, desktop, VS Code, Docker, Homebrew |
| Tag | **`v1.3.0`** (this cut) |
| Notes | [`RELEASE_NOTES_v1.3.0.md`](RELEASE_NOTES_v1.3.0.md) · [`CHANGELOG.md`](../CHANGELOG.md) |
| License | **Proprietary** NextEleven LLC — **not MIT**; `deny.toml` MIT allow = third-party deps only |
| `cargo test --bin harness` | **363** (prior cont) — re-run on tag commit |
| Coverage | **61.65%** · CI 60% **met** |
| CLAUDE.md | refreshed (363 / 61.65% / 1.3.0) |

**Go / no-go:** **GO** public POC. **Stable** still blocked on REL-01 + billing 📌. Prebuilt multi-arch may still need Actions billing.

---

**2026-08-09 — Cont residual (trust path-inject + wiring edges + docs honesty)**

| Item | Result |
|------|--------|
| Branch | **`main`** |
| Slice | `TrustStore::load_from_path` / `save_to_path` + pure trust edges; wiring SSE/LSP/mcp/swarm labels; TODO/CTO/COVERAGE_PLAN honesty post Swarm-51 |
| `cargo test --bin harness` | **363** pass (was 354 Swarm-51) |
| clippy `-D warnings` (bin harness) | clean |
| Residual | tui/mod entry · driver/input I/O loops · REL-01 Linux (Docker down on host) |
| Note | Discarded dirty accidental workspace version bump `0.1.2-beta`→`1.3.0-beta` (not shipped) |

**Go / no-go:** **GO** public beta. CI 60% still **met** (prior measure 61.65%). **Stable** still blocked on REL-01 + billing 📌.

---

**2026-08-09 — Public repo hardening (docs + scrub)**

| Item | Result |
|------|--------|
| Branch | **`main`** (visibility **PUBLIC**) |
| Secret scan | No real API keys / private keys in **tracked** tree or recent objects; local `.envrc` gitignored + never committed |
| Scrub | Machine home paths → `<repo-root>` in swarm notes / BENCHMARKS |
| License labels | Dockerfile, Homebrew, VS Code ext, CONTRIBUTING, SUBMISSION\* → proprietary / UNLICENSED (not MIT) |
| Docs honesty | README gates 363 / 61.65%; TEAM_UPDATE snapshot header; CHANGELOG Unreleased |
| Residual | `CLAUDE.md` edit blocked by agent protection this session — still shows stale 116/44.67% until maintainer patch |

**Go / no-go:** **GO** public beta. Rotate any local keys that lived only in untracked `.envrc` as defense-in-depth.

---

**2026-08-09 — Swarm-51 residual coverage climb (specialized agents ×50 + orch)**

| Item | Result |
|------|--------|
| Branch | **`main`** |
| `cargo test --bin harness` | **354** pass (was 264) |
| `cargo test -p harness-tools` | **179** pass (was 148) |
| clippy `-D warnings` (bin harness) | clean |
| llvm-cov workspace lines | **61.65%** (16497/26757); regions 63.37%; functions 68.04% |
| Climb slice | cost/cost_db · daemon · sync crypto · events · bridges · args clap · setup runtime · tools pure · observability path-inject · tui driver/render · Connect `--url` clap fix |
| Notes | `docs/swarm-51-2026-08-09/` · Vault `Vault/Swarm-51/` · durable id `swe184ad90` |
| Residual | tui/mod · main dispatch · wiring/driver/input I/O loops |

**Go / no-go:** **GO** public beta. **CI 60% coverage gate measured met.** **Stable** still blocked on full REL-01 matrix + billing prebuilts (📌).

---

**2026-08-07 — Coverage residual cont6 (memory_project + status/input titles)**

| Item | Result |
|------|--------|
| Branch | **`main`** |
| `cargo test --bin harness` | **264** pass (was 257) |
| clippy `-D warnings` (bin harness) | clean |
| llvm-cov workspace lines | **57.13%** (14212/24878); regions 58.60%; functions 63.80% |
| Climb slice | memory path-inject + sanitize; input_bar_title / status_indicators / status bar pad |
| Residual | draw_* · slash async I/O · build_tools_inner · tui/mod · target 60% open |

**Go / no-go:** **GO** public beta. **Stable** still blocked on full REL-01 matrix + billing prebuilts. CI 60% coverage still target (~57% measured).

---

**2026-08-07 — Coverage residual cont5 (slash parsers + event line kind)**

| Item | Result |
|------|--------|
| Branch | **`main`** |
| `cargo test --bin harness` | **257** pass (was 255) |
| clippy `-D warnings` (bin harness) | clean |
| llvm-cov workspace lines | **56.60%** (14010/24751); regions 58.04%; functions 63.19% |
| Climb slice | think/focus/remember/swarm-gc/schema/obsidian parsers; event_line_kind |
| Residual | draw_* · slash async I/O · build_tools_inner · tui/mod · target 60% open |

**Go / no-go:** **GO** public beta. **Stable** still blocked on full REL-01 matrix + billing prebuilts. CI 60% coverage still target (~57% measured).

---

**2026-08-07 — Coverage residual cont4 (notifications + cost format)**

| Item | Result |
|------|--------|
| Branch | **`main`** |
| `cargo test --bin harness` | **255** pass (was 252) |
| clippy `-D warnings` (bin harness) | clean |
| llvm-cov workspace lines | **56.31%** (13877/24646); regions 57.67%; functions 62.90% |
| Climb slice | notify pure copy builders; format_cost / format_tokens / unknown model |
| Residual | draw_* · slash async · build_tools_inner · tui/mod · target 60% open |

**Go / no-go:** **GO** public beta. **Stable** still blocked on full REL-01 matrix + billing prebuilts. CI 60% coverage still target (~56% measured).

---

**2026-08-07 — Coverage residual cont3 (theme + resume)**

| Item | Result |
|------|--------|
| Branch | **`main`** |
| `cargo test --bin harness` | **252** pass (was 245) |
| clippy `-D warnings` (bin harness) | clean |
| llvm-cov workspace lines | **56.18%** (13820/24600); regions 57.55%; functions 62.74% |
| Climb slice | theme load_from_str/path + parse_color + assistant_label; resume system/user/tool/turns |
| Residual | draw_* · slash-command async · build_tools_inner · tui/mod · target 60% open |

**Go / no-go:** **GO** public beta. **Stable** still blocked on full REL-01 matrix + billing prebuilts. CI 60% coverage still target (~56% measured).

---

**2026-08-07 — Coverage residual cont2 (slash + lightweight + input)**

| Item | Result |
|------|--------|
| Branch | **`main`** |
| `cargo test --bin harness` | **245** pass (was 237) |
| clippy `-D warnings` (bin harness) | clean |
| llvm-cov workspace lines | **55.60%** (13603/24468); regions 57.03%; functions 62.40% |
| Climb slice | slash `@file` expand_in + completions + pytest/go detect; lightweight MCP allowlist + config path; input search-nav + trust hint |
| Residual | handle_slash_command body · draw_* · build_tools_inner · target 60% open |

**Go / no-go:** **GO** public beta. **Stable** still blocked on full REL-01 matrix + billing prebuilts. CI 60% coverage still target (~56% measured).

---

**2026-08-07 — Coverage residual cont (wiring + render)**

| Item | Result |
|------|--------|
| Branch | **`main`** |
| `cargo test --bin harness` | **237** pass (was 227) |
| clippy `-D warnings` (bin harness) | clean |
| llvm-cov workspace lines | **55.05%** (13405/24350); regions 56.43%; functions 61.83% |
| Climb slice | wiring: confirm_policy / computer_use model / LSP markers / swarm label / mcp_names_added / SSE connect parse; render: wrap_text + compute_chat_items_from |
| Residual | build_tools_inner body · draw_* · tui/mod · input loops · target 60% open |

**Go / no-go:** **GO** public beta. **Stable** still blocked on full REL-01 matrix + billing prebuilts. CI 60% coverage still target (~55% measured).

---

**2026-08-07 — Coverage residual (confirm_flow + collab_ws + driver)**

| Item | Result |
|------|--------|
| Branch | **`main`** |
| `cargo test --bin harness` | **227** pass (was 212) |
| clippy `-D warnings` (bin harness) | clean |
| llvm-cov workspace lines | **54.51%** (13180/24179); regions 55.87%; functions 61.40% |
| Climb slice | confirm_flow hunk decide/move/finalize; collab `tool_result_preview` + `agent_event_to_collab` + user id; driver swarm id / user turns / fork_session |
| Residual | `cli/wiring` ~0%; `tui/mod` ~0%; input/render loops still thin; target 60% open |

**Go / no-go:** **GO** public beta. **Stable** still blocked on full REL-01 matrix + billing prebuilts. CI 60% coverage still target (~55% measured).

---

**2026-08-06 — Coverage climb (project_ops + TUI AppState)**

| Item | Result |
|------|--------|
| Branch | **`main`** @ `100188c`+ |
| `cargo test --bin harness` | **206** pass |
| clippy `-D warnings` (bin harness) | clean |
| llvm-cov workspace lines | **53.47%** (12770/23884); regions 54.89%; functions 60.35% |
| Climb slice | `parse_porcelain_counts` + default/allow test cmds + collect_files; AppState input/history/status |
| Residual | `tui/driver`, `tui/input`, `cli/wiring`, `server/collab_ws` still ~0% |

**Go / no-go:** **GO** public beta. **Stable** still blocked on full REL-01 matrix + billing prebuilts. CI 60% coverage still target (~53% measured).

---

**2026-08-05 — Swarm-50 complete (specialized agents)**

| Item | Result |
|------|--------|
| Branch | **`main`** |
| Iterations | **50/50** in `docs/swarm-50-2026-08-05/iterations/` |
| `cargo test --bin harness` | **190** pass |
| `cargo test -p harness-tools` | **148** pass |
| clippy `-D warnings` | clean |
| llvm-cov workspace lines | **51.98%** (12224/23516) |
| smoke_rel01 offline | PASS |
| Vault / HQ | `Vault/Swarm-50/` · HQ Projects/Harness + Activity Log |

---

**2026-08-05 — Swarm-50 start (docs honesty + residual climb)**

| Item | Result |
|------|--------|
| Branch | **`main`** @ `b29993f` (start HEAD) |
| Campaign | `docs/swarm-50-2026-08-05/` — 50 eng iterations; skip 📌 billing + keys-only |
| Baseline `cargo test --bin harness` | **123** pass (do not invent higher) |
| Coverage SoT | **51.98%** lines (`COVERAGE.md`; llvm-cov 2026-08-05 swarm-50) — 60% still target |
| Docs honesty slice | CTO exec findings: Gemini/Bedrock **closed**; coverage **51.98%**; Vault Index → `main` |
| Linux Docker smoke | **Not run** — Docker unavailable on this Mac host |
| Billing prebuilts | 📌 **PINNED** (W1.4–W1.5) |

**Go / no-go:** **GO** public beta (unchanged). **Stable** still blocked on full REL-01 matrix + billing prebuilts. Swarm-50 in progress toward coverage 60% + residual hygiene.

---

**2026-08-05 — Coverage remeasure + offline REL-01 subset**

| Item | Result |
|------|--------|
| Branch | **`main`** @ `b736ac0`+ |
| `cargo llvm-cov --workspace --all-features --summary-only` | **51.98%** lines (12224/23516); regions 53.37%; functions 58.96% |
| `cargo test --bin harness` | **123** pass |
| `cargo test -p harness-tools` | **110** pass |
| `bash scripts/smoke_rel01.sh` | **PASS** offline (doctor, swarm list/gc dry-run, mcp roots, sessions) |
| Linux Docker smoke (`smoke_linux_docker.sh`) | **Not run** — Docker unavailable on this Mac host |
| Key-dependent one-shot/TUI/export | **Still open** (W1.1) |
| Billing prebuilts | 📌 **PINNED** (W1.4–W1.5) |

**Go / no-go:** **GO** public beta. **Stable** still blocked on full REL-01 matrix + billing prebuilts. CI 60% coverage gate still a target (~47% measured).

---

**2026-07-30 — Collab max_users, checkpoint CI isolation, Mistral/OpenAI-compat**

| Item | Result |
|------|--------|
| Branch | `dev` → `origin/dev` |
| `cargo test --bin harness` collab/checkpoint | **Pass** |
| `cargo test -p harness-provider-router` | **Pass** (mistral + openai-compatible) |
| Features | collab rejoin seat fix; docs/COLLAB.md; Mistral + openai-compatible providers |
| CTO | W2.5 [x], W4.4 [x], W5.1 [x] |

**Go / no-go:** **GO** public beta. **Stable** still blocked on REL-01 + prebuilts.

---


**2026-07-30 — Quality floor + doctor/OTLP (llvm-cov 40%, cargo-deny green)**

| Item | Result |
|------|--------|
| Branch | `dev` → `origin/dev` |
| `cargo llvm-cov --workspace --all-features --summary-only` | **40.22%** lines (7814/19430) |
| `cargo deny check` | **Pass** (licenses + advisories + bans + sources) |
| `cargo clippy --workspace … -D warnings` | **Pass** (prior) |
| `harness doctor` | Bridges + observability sections |
| Docs | `COVERAGE.md`, `docs/OTLP_SMOKE.md`, badge ~40% |
| CTO | W2.1 [x]≥40%, W2.4 [x], W4.3 [x], W4.5 [x] |
| REL-01 / billing | **Unchanged** |

**Go / no-go:** **GO** public beta. **Stable** still blocked on REL-01 + prebuilts. CI 60% coverage gate still a target.

---

**2026-07-30 — Wave 2/3/4 eng slice (coverage, MCP CLI, notifications, clippy)**

| Item | Result |
|------|--------|
| Branch | `dev` → `origin/dev` |
| `cargo test --bin harness` | **Pass** — 96 tests |
| `cargo test -p harness-tools` | **Pass** — 54 tests |
| `cargo test -p harness-mcp` | **Pass** — 21 tests |
| `cargo test -p harness-memory` | **Pass** — 8 tests |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | **Pass** |
| Features | `harness mcp roots|resources|read`; spawn_agent desktop notify; COVERAGE_PLAN + NOTIFICATIONS_AUDIT |
| CTO items | W2.1 [~], W2.2 [x], W2.3 audit [~], W2.4 clippy [~], W3.7 [x], W4.2 [x] |
| REL-01 / prebuilt matrix | **Unchanged** — user keys/billing |

**Go / no-go:** **GO** for public beta (unchanged). **Stable** still blocked on REL-01 + full prebuilts/Homebrew.

---

**2026-07-30 — Swarm operability + `dev` branch (local + GitHub)**

| Item | Result |
|------|--------|
| Branch | **Done** — `dev` tracking `origin/dev` |
| Feature commit | **Done** — `0c7d59e` feat(swarm): TUI panel, orphan GC, richer status CLI |
| `cargo test --bin harness` | **Pass** — 84 tests |
| Swarm unit tests | **Pass** — 11 tests |
| `cargo clippy --bin harness -- -D warnings` | **Pass** |
| Team brief | [`TEAM_UPDATE_2026-07-30.md`](TEAM_UPDATE_2026-07-30.md) |
| Roadmap | [`ROADMAP.md`](ROADMAP.md) |
| REL-01 / prebuilt matrix | **Unchanged** — still stable blockers |

**Go / no-go:** **GO** for public beta (unchanged). **Stable** still blocked on REL-01 + full prebuilts/Homebrew. Swarm daily-driver path is materially better on `dev`.

---

**2026-05-25 — v0.1.2-beta release cut (local + partial GitHub Release)**

| Item | Result |
|------|--------|
| Version bump | **Done** — `0.1.2-beta` (`f1ab11a`) |
| Tag | **Done** — `v0.1.2-beta` pushed |
| Automated gates (local) | **Pass** — fmt, clippy, 218 tests, release-lto, `smoke_rel01.sh` |
| GitHub Release workflow | **Blocked** — account billing lock; jobs did not start |
| GitHub Release (manual) | **Partial** — [v0.1.2-beta](https://github.com/seanebones-lang/harness/releases/tag/v0.1.2-beta) with **macOS arm64** binary only |
| Homebrew formula | **Partial** — macOS arm64 SHA in `homebrew/harness.rb`; x64/Linux placeholders remain |
| REL-01 macOS (automated + local) | **Partial pass** — export, `serve` health, bearer auth 401/200; one-shot/TUI need API keys |
| REL-01 Linux / Windows | **Pending** — maintainer on target OS |

**Blocker:** Resolve GitHub Actions billing, then re-run Release workflow or upload remaining platform binaries and run `bash scripts/update-homebrew-sha.sh v0.1.2-beta`.

**Go / no-go:** **GO** for public beta (source + macOS arm64 prebuilt). **Stable** blocked on full REL-01 per OS + complete prebuilt matrix.

---

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

**2026-08-03 — Max-opt swarm (W5.2–W5.7 + quality)**

| Item | Result |
|------|--------|
| Branch | `dev` |
| `cargo build --bin harness` | **Pass** |
| `cargo clippy -p harness --bin harness -- -D warnings` | **Pass** |
| `cargo test -p harness-tools` | **95 pass** |
| Providers | Gemini OpenAI-compat + Bedrock Converse; router tests green |
| Tools | `database` / `notebook` / `docker` config-gated (default off) |
| Docs | PROVIDERS_GEMINI_BEDROCK.md, COMPUTER_USE.md, WAVE7_SCALE.md |
| Offline smoke | smoke_rel01 + scripts/smoke_linux_docker.sh |
| CTO | W5.2–W5.7 [x]; W1.4–W1.5 still 📌 |

**Go / no-go:** **GO** public beta. **Stable** still blocked on REL-01 full matrix + billing prebuilts.

---

**2026-08-03 — Single-branch main + full docs refresh**

| Item | Result |
|------|--------|
| Branch | **`main` only** (`dev` merged + deleted) |
| `cargo test --bin harness` | **116 pass** |
| Docs | Full README rewrite; CLAUDE/TODO/SHORTCUTS/CTO/TEAM_UPDATE aligned |
| License | Proprietary NextEleven LLC (LICENSE on main) |
| Coverage badge | ~45% measured (COVERAGE.md 44.67%) |

**Go / no-go:** **GO** public beta. **Stable** still blocked on REL-01 + prebuilts.

---

## Current recommendation (public beta)

| Item | Status |
|------|--------|
| **License** | Proprietary NextEleven LLC (`LICENSE`) |
| **Automated gates** | **190** bin tests; clippy clean on ship; CI multi-OS; coverage **target** 60% (measured **51.98%** / ~52%) |
| **P0 security** | **Closed** — see [`PEER_REVIEW_AUDIT.md`](PEER_REVIEW_AUDIT.md) |
| **Threat model** | [`docs/THREAT_MODEL.md`](THREAT_MODEL.md) v2 |
| **Open backlog** | [`TODO.md`](../TODO.md) · [`CTO_BACKLOG.md`](CTO_BACKLOG.md) |
| **Manual smoke §3** | **Pending** — blocks **stable** |
| **Ship branch** | **`main`** |

**Verdict:** **GO** for **public beta**. Promote to **stable** only after **REL-01** manual smoke §3 on target OSes (record above).

---

_Update this file when you tag a release or change licensing._
