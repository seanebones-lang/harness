# Harness — Backlog & Roadmap (May 2026)

**Canonical user docs:** [`README.md`](README.md) · [`Start Here/USER MANUAL.md`](Start%20Here/USER%20MANUAL.md)  
**Developer map:** [`CLAUDE.md`](CLAUDE.md) · [`CONTRIBUTING.md`](CONTRIBUTING.md)  
**Security / audit:** [`docs/PEER_REVIEW_AUDIT.md`](docs/PEER_REVIEW_AUDIT.md) · [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md)  
**Release:** [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) · [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md)

---

## Current state (May 2026)

| Area | Status |
|------|--------|
| **Release verdict** | **Public beta GO** — stable blocked on manual smoke §3 |
| **Automated gates** | `cargo test --all` (**176 tests**), `clippy -D warnings`, CI multi-OS, coverage ≥ 60% on PRs |
| **P0 security** | **Closed** — tar-slip sync, HTTP/daemon bearer auth, confirm gate, tool sandboxing, git force-push guard |
| **Agent loop** | Max 50 tool rounds; integration tests; `ContextCompacted` event; model-aware compaction |
| **Providers** | Anthropic, xAI, OpenAI, Ollama, MLX; router with fallback; SSE tests for Anthropic/Ollama |
| **Tools** | Workspace sandbox on filesystem/search/git/apply_patch; plan-mode confirm gate; 32+ harness-tools tests |
| **Editor IPC** | Daemon length-prefixed frames + token; `[daemon].transport` wired; VS Code extension aligned |
| **Experimental (compiled, not wired)** | OTLP export tested via mock HTTP; full Jaeger/Tempo E2E optional |
| **Swarm** | cancel/wait CLI; `[swarm]` in Config |
| **Collab** | WebSocket when `[collab].enabled` |
| **Bridges** | `harness bridge` CLI |
| **Threat model** | [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md) |

---

## Open work — ranked by severity

### 🔴 Critical (blocks stable release)

| ID | Task | Owner hint | Notes |
|----|------|------------|-------|
| **REL-01** | **Manual smoke §3** on macOS, Linux, Windows | Maintainers | Checklist in [`PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) §3 — one-shot, TUI, `serve` + bearer auth, daemon/VS Code, export, `--plan`. Record results in [`RELEASE_STATUS.md`](docs/RELEASE_STATUS.md). |

### 🟠 High (security / product honesty — next sprint)

_None — E-03 closed May 2026._

### 🟡 Medium (quality / completeness)

| ID | Task | Notes |
|----|------|-------|
| **AGT-06** | Move `tool_calls_to_message` to `harness-provider-core` | ✅ shared helper + unit test |
| **SWARM-01** | Implement `cancel` / `wait` on swarm tasks **or** keep docs as-is | ✅ `harness swarm cancel/wait` |
| **SWARM-02** | Wire `[swarm]` block in `Config` (today comment stub in `default.toml`) | ✅ `max_concurrency`, `db_path` |
| **TST-10** | Proptest: MCP NDJSON classifier, LSP async read path | Partial proptest exists elsewhere |
| **TST-11** | Raise coverage gate **60% → 70%** | [`coverage.yml`](.github/workflows/coverage.yml) |
| **TST-12** | `checkpoint.rs` unit tests | Git stash integration untested |
| **OBS-01** | OTLP export integration test (Jaeger/Tempo or mock) | ✅ mock axum `/v1/traces` test |
| **REL-02** | VS Code extension **E2E on native Windows** | Protocol + token fixed; no automated E2E |
| **REL-04** | Wire `[daemon] transport` in TOML → `Config` | ✅ `auto` / `unix` / `tcp` |
| **CFG-01** | Wire `[approval].effective_mode()` or remove dead code | `config.rs` |
| **CFG-02** | Add `[collab]` / `[bridges]` to `Config` struct **or** remove from `default.toml` | ✅ wired in `Config` + `default.toml` |

### 🟢 Low (polish / docs)

| ID | Task | Notes |
|----|------|-------|
| **DOC-06** | CDP doc screenshots (`docs/images/`) | [`BROWSER_CDP.md`](docs/BROWSER_CDP.md) |
| **DOC-07** | Complete Spanish manual | [`docs/i18n/USER_MANUAL.es.md`](docs/i18n/USER_MANUAL.es.md) partial |
| **REL-03** | Tauri **Windows/Linux** packaging + auto-update | macOS `.app` exists |
| **ARCH-01** | Ambient `spawn` trigger test (`interval = 1ms`, `min_new = 2`) | Nice-to-have |
| **ARCH-02** | Optional fast model for consolidation via `router.fast_model()` | Config field exists |

### ⚪ Optional (post-stable)

| ID | Task |
|----|------|
| **TL-01** | `DatabaseTool` — SQLite/Postgres → markdown |
| **TL-02** | `NotebookTool` — Jupyter `.ipynb` |
| **TL-03** | `DockerTool` — container list/exec/logs |
| **PROV-01** | New providers: Mistral, Gemini, Bedrock (see CLAUDE.md) |
| **ENT-01** | HTTP SSO, audit log, team policy profiles |

---

## Roadmap

### ✅ Phase A — Safety hardening (complete May 2026)

- Sandbox on filesystem, search, git, apply_patch
- Confirm gate fail-closed; plan mode; MCP in gate; sub-agent gate
- HTTP + daemon bearer tokens; threat model doc
- Sync tar-slip fix; CSPRNG passphrase; config/shell log 0600

### ✅ Phase B — Core reliability (complete May 2026)

- Agent loop tests; 50-round cap; error propagation to HTTP SSE
- Anthropic/Ollama/router/provider-core tests; Ollama multi-tool fix
- `native_tools`, `--think`, `[approval]` wired
- 164 automated tests (was ~114 at audit start)

### 🔄 Phase C — Product completeness (in progress, ~3–6 months)

| Milestone | Status | Deliverable |
|-----------|--------|-------------|
| C.1 Collab | **Done** | WebSocket `/ws/session/:id` when `[collab].enabled` |
| C.2 Bridges | **Done** | `harness bridge` CLI |
| C.3 Swarm | **Done** | cancel/wait + `[swarm]` config |
| C.4 Diff review | **Done** | Hunk overlay in `--plan` TUI for write/patch |
| C.5 Observability | **Partial** | Local JSONL ✅; OTLP mock HTTP test ✅ |
| C.6 Stable release | **Blocked** | REL-01 manual smoke on target OSes |

### 📅 Phase D — Ecosystem & scale (6–12 months)

| Milestone | Deliverable |
|-----------|-------------|
| D.1 Providers | Mistral, Gemini, Bedrock |
| D.2 Desktop | Cross-platform Tauri installers; tray; auto-update |
| D.3 Enterprise | SSO, audit log, policy profiles |
| D.4 Performance | Cache dashboard; embed batching; smarter compaction |
| D.5 i18n | Full manual translation; locale-aware TUI |

---

## Recently completed

- **AGT-06** — `tool_calls_to_message` in `harness-provider-core` (shared wire format)
- **OBS-01** — OTLP export integration test (mock axum `/v1/traces`)
- **REL-04** — `[daemon].transport` wired (`auto` / `unix` / `tcp`)
- **E-03** — Diff review hunk overlay in plan-mode TUI (`write_file` / `patch_file`)

### Phase C follow-up (2026-05-22)

- **E-01** — Collab WebSocket + `[collab]` in Config
- **E-02** — `harness bridge` CLI
- **SWARM-01/02** — `cancel`/`wait`; `[swarm]` config

### Peer review follow-up (2026-05-22)

- **AGT-07** — `harness-tools::policy` unifies checkpoint + plan-mode lists (incl. git mutating ops)
- **SEC-14** — HTTP rate limit (60/min/IP) on protected routes; non-loopback bind warning
- **SEC-15** — Optional `[mcp].command_allowlist` in config
- **REL-05** — `cargo build --profile release-lto` pass (2026-05-22)

### Peer review remediation (2026-05-22)

See full table in [`docs/PEER_REVIEW_AUDIT.md`](docs/PEER_REVIEW_AUDIT.md#remediation-applied-2026-05-22-session).

Highlights: P0×7 closed; agent/swarm/tool/provider tests; VS Code daemon protocol; computer-use hardening; `docs/THREAT_MODEL.md`; `config/default.toml` prompt aligned with `DEFAULT_SYSTEM`.

### May 2026 backlog (pre-audit)

- Ambient `ArcProvider` + consolidation tests; browser tests; MemoryStore tests
- Session title fixes; Windows PowerShell shell; daemon TCP; Tauri serve autospawn
- Proptest (MCP, LSP, OpenAI/xAI SSE); coverage CI; `missing_docs` on core crates
- Docs: `BROWSER_CDP.md`, `COOKBOOK.md`, Spanish manual excerpt

---

## Automated gates (run before PR)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo build --profile release-lto   # before release tag
```
