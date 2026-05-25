# Peer Review Audit — harness (May 2026)

**Audience:** Core maintainers, security reviewers, and prospective contributors  
**Scope:** Full-workspace read of ~136 Rust/TOML/CI sources; automated gate verification; security surface analysis  
**Auditor posture:** Pre-release peer review — findings are actionable, severity-ranked, and tied to file references  
**Verdict:** **Public beta GO** — **stable** blocked on **REL-01** (manual smoke §3). P0 closed; **218 tests**; threat model published.

### Current application state (May 2026)

| Layer | What ships today |
|-------|------------------|
| **CLI / TUI** | Multi-provider chat, plan mode, extended thinking, sessions, export, cost tracking, swarm CLI |
| **Agent** | Tool loop (cap 50), memory RAG, compaction + `ContextCompacted` event, project facts (`.harness/memory/`) |
| **Security** | Workspace sandbox; HTTP/daemon bearer tokens; confirm gate; sync tar-slip fix — see [`THREAT_MODEL.md`](THREAT_MODEL.md) |
| **Providers** | Anthropic, xAI, OpenAI, Ollama, MLX; smart router + fallback |
| **Tools** | Filesystem, shell, git, search, apply_patch, MCP, browser (CDP), LSP, gh, test_runner, computer-use (gated) |
| **Integrations** | VS Code daemon (framed JSON + token); HTTP `serve` + Web UI; Tauri macOS shell |
| **Swarm** | `cancel`/`wait` CLI; `[swarm]` config; SQLite persistence |
| **Collab** | WebSocket `/ws/session/:id` when `[collab].enabled` |
| **Bridges** | `harness bridge` CLI (Obsidian, Notes, Calendar, GitHub Projects) |
| **Not wired** | OTLP verification |

**Canonical open backlog (severity-ranked):** [`TODO.md`](../TODO.md)

### Remediation applied (2026-05-22 session)

| ID | Status | Notes |
|----|--------|-------|
| P0-1 Tar-slip sync | **Fixed** | `entry.unpack_in` + `validate_tar_entry_path`; tests |
| P0-2 ConfirmGate fail-open | **Fixed** | Deny on closed/full/dropped reply; 3 unit tests |
| P0-3 `confirm_required` | **Fixed** | Enforced in `ShellTool::execute` |
| P0-4 `apply_patch` exfil | **Fixed** | Paths resolved via `WorkspaceRoot` before read |
| P0-5 Git HEAD force-push | **Fixed** | Resolves symbolic ref; blocks main/master |
| P0-6 HTTP auth + RCE surface | **Fixed** | Bearer token (`~/.harness/server.token`); protected routes; test cmd allowlist; agent errors surfaced |
| P0-7 Daemon auth | **Fixed** | Token in `~/.harness/daemon.token`; verified per request |
| P1 search/git sandbox | **Fixed** | `SearchCodeTool` + `GitTool` workspace-bound |
| P1 MCP confirm + roots | **Fixed** | MCP tools in plan gate; home root removed |
| P1 sub-agent gate | **Fixed** | Confirm gate propagated to sub-agents |
| P1 sync passphrase | **Fixed** | UUID v4 CSPRNG |
| P1 agent loop cap | **Fixed** | Max 50 tool rounds |
| P1 native_tools / --think | **Fixed** | Wired through TUI, `run_once`, HTTP serve |
| P1 config 0600 | **Fixed** | `write_config_toml` + shell log permissions |
| P1 status key redaction | **Fixed** | No partial key display |
| P1 Anthropic SSE tests | **Fixed** | 3 parser tests; cache usage emitted on `message_start` |
| P1 Ollama multi-tool | **Fixed** | `pending_tools` queue + EOF flush; 2 tests |
| P1 computer-use injection | **Fixed** | Input validation + AppleScript escaping; 3 tests |
| P1 VS Code daemon protocol | **Fixed** | Length-prefixed frames + `daemon.token` auth |
| P1 collab/bridges honesty | **Partial** | Module docs mark EXPERIMENTAL; not wired |
| P1 agent loop tests | **Fixed** | 3 integration tests in `src/agent.rs` |
| P1 provider-router tests | **Fixed** | 3 tests: routing, fallback, from_config |
| P1 swarm persistence | **Fixed** | 4 tests; failed-status bug; UUID task ids |
| P1 SessionStore tests | **Fixed** | 4 in-crate tests in `harness-memory` |
| P1 provider-core serde | **Fixed** | 6 roundtrip tests in `types.rs` |
| P1 tool sandbox tests | **Fixed** | filesystem, apply_patch, git, spawn_agent (10 tests) |
| P1 compaction/memory tests | **Fixed** | 4 tests + `ContextCompacted` event + model-aware limits |
| P1 threat model doc | **Fixed** | `docs/THREAT_MODEL.md` |
| P1 doc/config honesty | **Fixed** | swarm API, collab, default.toml prompt, CLAUDE.md |

**Still open (see Part III):** collab/bridges full wiring or removal, manual smoke §3 (checklist ready in PUBLIC_RELEASE §3).

---

## Executive summary

**harness** is a well-structured Rust coding agent: clean workspace layout, strong CI (multi-OS, supply-chain, MSRV, 60% coverage gate), and thoughtful primitives (workspace sandbox, plan mode, MCP 2025-03-26, provider abstraction). The May 2026 remediation wave closed **all seven P0 findings**, raised automated tests from ~114 to **218** (Round 2), and published a threat model.

Several E-phase modules (`collab`, `bridges`, `diff_review`) remain compiled but unwired — acceptable for **public beta**, not for **stable** until resolved or removed.

**Recommendation:** **Public beta GO.** Promote to **stable** only after [`REL-01`](../TODO.md) manual smoke §3 on target OSes.

### Automated gates (this workspace, 2026-05-22)

| Gate | Result |
|------|--------|
| `cargo test --all` | **Pass** — **218 tests** |
| `cargo clippy --all-targets --all-features -- -D warnings` | **Pass** |
| `cargo fmt --all -- --check` | Not re-run this session (prior pass) |
| Manual smoke §3 | **Pending** — checklist in [`PUBLIC_RELEASE.md`](PUBLIC_RELEASE.md) §3 |
| P0 security | **Closed** |
| Threat model | [`THREAT_MODEL.md`](THREAT_MODEL.md) |

---

## Part I — Critical fixes (P0) — ✅ CLOSED May 2026

All items below were remediated in the 2026-05-22 session. Kept for audit trail.

### P0-1 — Tar-slip on sync pull ✅

**File:** `src/sync.rs:191–195`, `377–382`  
**Issue:** `untar_dir` calls `tar::Archive::unpack(parent)` with no path sanitization. A malicious `memory.tar.age` in the sync git remote can write outside `~/.harness/` (e.g. `../../.ssh/authorized_keys`).  
**Fix applied:** `validate_tar_entry_path` + `entry.unpack_in`; regression tests in `src/sync.rs`.

### P0-2 — ConfirmGate fail-open ✅

**File:** `crates/harness-tools/src/confirm.rs:36–44`  
**Issue:** When the confirm channel is closed (`TrySendError::Closed`), `request()` returns `true` (approve). A crashed TUI or headless path silently disables plan-mode protections. `rx.await.unwrap_or(true)` has the same fail-open semantics.  
**Fix applied:** Deny on closed/full/dropped channel; 3 unit tests in `confirm.rs`.

### P0-3 through P0-7 ✅

| ID | Fix applied |
|----|-------------|
| P0-3 | `confirm_required` enforced in `ShellTool::execute` |
| P0-4 | `apply_patch` resolves paths via `WorkspaceRoot` before read |
| P0-5 | Git push resolves `HEAD` via `symbolic-ref`; blocks main/master force-push |
| P0-6 | HTTP bearer auth (`server.token`); protected routes; SSE errors; test cmd allowlist |
| P0-7 | Daemon token (`daemon.token`); verified per request; VS Code uses framed protocol |

---

## Part II — P1 remediation — mostly closed

| ID | Status | Notes |
|----|--------|-------|
| P1-1–P1-3 | ✅ | MCP in confirm gate; home root removed from MCP roots |
| P1-4–P1-6 | ✅ | Search/git sandbox; sub-agent gate |
| P1-7–P1-8 | ✅ | Config 0600; sync UUID passphrase |
| P1-9 | ✅ | Computer-use input validation |
| P1-10–P1-13 | ✅ | Tool loop cap; native_tools; `--think`; approval plan mode |
| P1-11 | ✅ | **AGT-07** — shared `policy` module; git mutating ops included |
| P1-14 | ✅ | **AGT-06** — `tool_calls_to_message` in provider-core |
| P1-2 (partial) | ✅ | **SEC-15** — `[mcp].command_allowlist` |
| SEC-14 | ✅ | Rate limit 60/min/IP; non-loopback bind warning |

---

## Part III — Open backlog (severity-ranked)

**Full list with roadmap:** [`TODO.md`](../TODO.md)

### 🔴 Critical — blocks stable

| ID | Task |
|----|------|
| **REL-01** | Manual smoke §3 on macOS, Linux, Windows → record in [`RELEASE_STATUS.md`](RELEASE_STATUS.md) |

### 🟠 High — next sprint

_None — E-03 closed May 2026._

### 🟡 Medium

| ID | Task |
|----|------|
| **SWARM-01/02** | ✅ `cancel`/`wait`; `[swarm]` in Config |
| **TST-10/11/12** | ✅ MCP/LSP proptest; coverage 70% blocked (~48% lib baseline); checkpoint tests ✅ |
| **OBS-01** | ✅ OTLP mock HTTP integration test |
| **REL-02** | VS Code Windows E2E |

### 🟢 Low / ⚪ Optional

See [`TODO.md`](../TODO.md) — DOC-06/07, Tauri packaging, new tools (TL-01–03), new providers (PROV-01).

### Completed checklist (reference)

<details>
<summary>SEC / AGT / TST / DOC items closed in remediation</summary>

- [x] SEC-01–SEC-13 (P0/P1 security wave)
- [x] AGT-01–AGT-05, AGT-08, AGT-09
- [x] TST-01–TST-09
- [x] DOC-01–DOC-05

</details>

---

## Part IV — Roadmap

### Phase A — Safety hardening ✅ (complete May 2026)

Sandbox, confirm gate, HTTP/daemon auth, sync tar-slip, [`THREAT_MODEL.md`](THREAT_MODEL.md).

### Phase B — Core reliability ✅ (complete May 2026)

**218 tests**; agent loop bounded + tested; provider SSE tests; `native_tools` / `--think` / approval wired.

### Phase C — Product completeness 🔄 (in progress)

| Milestone | Status |
|-----------|--------|
| C.1 Collab | **Done** — `/ws/session/:id` when `[collab].enabled` |
| C.2 Bridges | **Done** — `harness bridge` CLI |
| C.3 Swarm | **Done** — cancel/wait + `[swarm]` config |
| C.4 Diff review | **Done** — plan-mode hunk overlay |
| C.5 Observability | **Partial** — JSONL ✅; OTLP untested |
| **C.6 Stable** | **Blocked on REL-01** manual smoke |

### Phase D — Ecosystem & scale (6–12 months)

New providers; cross-platform desktop; enterprise features; performance; i18n.

**Full tables:** [`TODO.md`](../TODO.md#roadmap)

---

## Part V — Detailed findings by category

### 5.1 Architecture strengths

1. **Workspace layout** — Clear separation: root binary, 14 crates, config, docs, CI. Provider trait in `harness-provider-core` is the right abstraction.
2. **CI maturity** — Multi-OS matrix, supply-chain (`cargo audit`, `cargo deny`), MSRV 1.76, install script smoke, coverage gate on PRs.
3. **Filesystem sandbox** — `WorkspaceRoot` with strict/relaxed/off modes; symlink escape tested (`workspace_root.rs:244–263`).
4. **Tool executor** — Plan mode, trusted patterns, autoformat/autotest hooks are well-designed primitives.
5. **MCP client** — Dedicated stdout reader, sampling approval path, in-process concurrency tests.
6. **Provider streaming** — OpenAI/xAI have proptest-hardened SSE parsers; retry with exponential backoff.
7. **Public API discipline** — `#![deny(missing_docs)]` on core crates.

### 5.2 Security posture (post-remediation)

| Sev | Original count | May 2026 status |
|-----|----------------|-----------------|
| Critical (P0) | 7 | **All closed** |
| High (P1) | 14 | **14 closed** |
| Residual | — | Rate limiting; MCP allowlist; experimental modules unwired |

### 5.3 Correctness & reliability (updated)

| Issue | Status |
|-------|--------|
| No max tool-loop iterations | ✅ Cap at 50 |
| Ollama multi-tool per chunk | ✅ Fixed + tested |
| HTTP swallows agent errors | ✅ SSE error events |
| Context compaction silent | ✅ `ContextCompacted` event |
| Malformed patch panics | ✅ Fixed — parser returns `Err` on bad hunk headers; regression test |

### 5.4 Dead code & doc drift (updated May 2026)

| Module / flag | Status |
|---------------|--------|
| `src/collab.rs` | ✅ Wired when `[collab].enabled` |
| `src/bridges.rs` | ✅ `harness bridge` CLI |
| `src/diff_review.rs` | ✅ Plan-mode TUI overlay (E-03) |
| `src/observability.rs` | Local JSONL + OTLP mock test |
| Swarm cancel/wait | ✅ Implemented |
| `[collab]`, `[swarm]`, `[bridges]`, `[daemon]`, `[approval]` | ✅ In `Config` + `default.toml` |

### 5.5 Test coverage snapshot

| Area | Tests | Gap |
|------|------:|-----|
| Workspace total | **218** | — |
| `harness-tools` (incl. tool modules) | 32+ | Strong |
| `harness-mcp` | 10 | Good |
| `harness-provider-openai` | 9 | Good |
| `harness-browser` | 9 | Good |
| `src/agent.rs` | 9 | Good — compaction + memory tests |
| `harness-provider-anthropic` | 3 | Medium — extend for thinking blocks |
| `harness-provider-ollama` | 2 | Medium |
| `harness-provider-router` | 3 | Medium |
| `harness-provider-core` | 6 | Good |
| `src/swarm.rs` | 4 | Good |
| `harness-memory` (SessionStore) | 4 | Good |

---

## Part VI — Release gate checklist (peer sign-off)

| Criterion | Beta | Stable |
|-----------|------|--------|
| `cargo test --all` | ✅ | Required |
| `cargo clippy -D warnings` | ✅ | Required |
| Coverage ≥ 60% | ✅ CI | ≥ 70% recommended |
| P0 security fixes | ✅ closed | All closed |
| Manual smoke §3 | ❌ pending | Required all target OS |
| Config flags match behavior | ⚠️ partial | collab/bridges/swarm stubs |
| Dead E-phase modules | ⚠️ tolerated | Wire or remove for stable |
| Threat model documented | ✅ | Required |
| Agent loop tested | ✅ | Required |

**Peer sign-off statement (template):**

> We reviewed harness after the 2026-05-24 Round 2 re-inspection. Automated gates pass (**218 tests**). All P0 items remain closed. Round 2 P1 fixes (bridges, apply_patch, health leak, router panic, unwrap hardening) are applied. We approve **public beta**. We **do not** approve **stable** until REL-01 manual smoke is recorded on target OSes.

---

---

## Round 2 — MIT Re-Inspection (2026-05-24)

**Baseline:** `433065d` on `main` (post-audit batch: gh `pr_create`, MCP sampling gate, session titles, TUI follow-scroll, desktop CI, term-graphics tests).  
**Scope:** Re-run automated gates; fix actionable P1–P2 regressions and gaps; refresh docs. Excluded maintainer-only REL-01 full manual smoke and Homebrew tap publish.

### Automated gates (Round 2)

| Gate | Result |
|------|--------|
| `cargo fmt --all -- --check` | **Pass** |
| `cargo clippy --all-targets --all-features -- -D warnings` | **Pass** |
| `cargo test --all` | **Pass** — **218 tests** |
| `cargo build --profile release-lto` | **Pass** |
| `scripts/smoke_rel01.sh` | **Pass** (automated subset) |
| CI `smoke-rel01` job | **Added** — ubuntu, post-build |

### Round 2 findings — remediated

| ID | Severity | Finding | Fix |
|----|----------|---------|-----|
| R2-1 | P1 | GitHub Projects bridge: GraphQL body never written to `gh api graphql` stdin | `github_projects_graphql_body()` + stdin write; serde_json parameterization |
| R2-2 | P1 | Apple Notes bridge: incomplete AppleScript escaping | Reuse `escape_applescript()` (parity with calendar) |
| R2-3 | P1 | `apply_patch` parser `unwrap()` on malformed hunks | `Err` paths + `parse_malformed_hunk_returns_err_not_panic` test |
| R2-4 | P2 | `/api/health` leaks `config_path` to non-loopback clients | Gate behind loopback check (same as `auth_token`) |
| R2-5 | P1 | `ProviderRouter::default_provider().expect(...)` panic on empty map | Returns `Option`; safe fallbacks in trait methods |
| R2-6 | P2 | Production `unwrap()` in rate_limit, swarm, diff_review, browser | Poison → deny; explicit branches |
| R2-7 | P2 | Weak test coverage (voice, mlx, lsp detect, collab) | Unit tests added |
| R2-8 | P2 | REL-01 subset not in CI; release checksum fragility; Windows install asymmetry | `smoke-rel01` CI job; explicit `sha256sum`; `install.ps1` prebuilt download |

### Round 2 — still open (maintainer-only)

| ID | Item |
|----|------|
| **REL-01** | Full manual smoke §3 — API keys + TUI + serve + export on target OSes |
| **P2-10** | Homebrew tap SHA — run `scripts/update-homebrew-sha.sh v0.1.1-beta` after next tag |

### Round 2 verdict

**Public beta GO** · **Stable NO-GO** until REL-01 logged in [`RELEASE_STATUS.md`](RELEASE_STATUS.md).

---

## Appendix A — Methodology

1. Read all 136 workspace source files (Rust, TOML, CI YAML, key docs).
2. Ran `cargo test --all` and `cargo clippy --all-targets --all-features -- -D warnings`.
3. Grep for `unwrap`, `expect`, `TODO`, `dead_code`, config wiring gaps.
4. Traced tool execution path: `agent.rs` → `ToolExecutor` → individual tools → sandbox.
5. Traced network path: `server.rs`, `daemon.rs`, `collab.rs`, MCP spawn.
6. Cross-checked CLAUDE.md, TODO.md, RELEASE_STATUS.md against implementation.

## Appendix B — Positive security controls (preserve)

- `persist_setup` loopback-only write restriction (`server.rs:238–247`)
- Daemon frame size cap 64 MiB (`daemon.rs:156–157`)
- Git pull `--ff-only` in structured tool (`git.rs:168–171`)
- Filesystem tools consistently use `WorkspaceRoot::resolve` in strict mode
- MCP sampling denial when approval callback returns false

---

*Generated for peer review. Update this document when P0 items close or release gates change.*
