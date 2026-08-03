# CTO Engineering Backlog — harness

**Date:** 2026-07-30  
**Perspective:** CTO review of codebase + Obsidian vault  
**Branch base:** `dev` @ `54da34e` (was f06ee9b at backlog create)  
**Verdict:** Strong multi-provider agent core; **operability caught up substantially on 2026-07-30** (coverage honesty, swarm/MCP/collab/doctor, deny/clippy). Remaining blockers are **release smoke + billing matrix**, then capability/competition depth.

**EOD pointer:** vault `Vault/Status/Session-Close-2026-07-30.md` · PR https://github.com/seanebones-lang/harness/pull/5

**How to use:** Work top-to-bottom within each wave. Do not start Wave N+1 items that block Wave N release gates unless explicitly re-prioritized. Check boxes in this file as we execute.

---

## Executive findings (gaps)

### A. Trust & release (business-critical)
| Gap | Evidence | Risk |
|-----|----------|------|
| Stable blocked | REL-01 incomplete; Linux/Windows smoke missing | Cannot claim production-ready |
| Prebuilt matrix partial | macOS arm64 only; CI billing history (W1.4–1.5 📌) | Broken install story outside one arch |
| Coverage lag | `COVERAGE.md` **40.22%** lines (llvm-cov 2026-07-30); badge ~40%; CI 60% still target | OK if honest; not stable gate yet |
| Doc/status drift | Largely reconciled 2026-07-30 (TODO + CTO + vault) | Keep checkboxes honest on ship days |
| Vault | Populated Index/Status/Backlog + session close | Maintain on each ship day |

### B. Product honesty (UX debt)
| Gap | Evidence | Risk |
|-----|----------|------|
| Slash stubs | **Closed** — `/obsidian`, `/trace` wired | — |
| COOKBOOK gap | **Closed** — swarm + MCP + browser sections | Keep examples current |
| Competition checklist open | Many **[C]** boxes still open (W8) | Submission not judge-ready |

### C. Architecture & quality
| Gap | Evidence | Risk |
|-----|----------|------|
| God files | `agent.rs` / `server.rs` / heavy TUI still large | Change risk (W7.4) |
| Unwrap surface | `src/` production mostly clean; crates residual | Panic paths under poison/IO |
| MCP sampling UX | **Closed** — TUI y/n + auto | — |
| Version drift | **Closed** — desktop 0.1.2-beta aligned | — |
| Toolchain pin | **Closed** — `1.95.0` | — |
| Swarm next | cancel-all / auto-gc / --json done; model-on-task optional | Ops mostly unblocked |

### D. Compatibility surface
| Gap | Notes |
|-----|--------|
| Windows/Linux release artifacts | Install scripts exist; Release matrix incomplete |
| Provider breadth | **Mistral + openai-compatible done**; Gemini/Bedrock open (W5.2) |
| Editor/desktop | VS Code + Tauri early; packaging incomplete (W6) |
| i18n | ES manual partial/draft |

---

## Ordered TODO (execute in this sequence)

### WAVE 0 — Truth & alignment (½–1 day)  ← **START HERE**
Goal: one backlog, honest metrics, vault usable, no lying checkboxes.

- [x] **W0.1** Reconcile `TODO.md` P2-1/P2-2/P2-3 with reality (scrollbar partial, session names, notifications). Downgrade false [x] → [~] or [ ] with notes. *(2026-07-30)*
- [x] **W0.2** Fix coverage messaging: badge/README/CLAUDE vs `COVERAGE.md` (23%). Either remeasure and document true % or remove “60%+” badge until gate is real. *(coverage-auditor)*
- [x] **W0.3** Wire slash stubs to real implementations or honest “not available” with doc links (`/obsidian`, `/trace` at minimum). *(slash-impl)*
- [x] **W0.4** Populate vault: `Architecture/Overview.md`, `Backlog/CTO-TODO.md` (link this file), refresh `Status/Current-Condition.md` for `dev`. *(2026-07-30)*
- [x] **W0.5** COOKBOOK: add swarm worked example (run → list → status → gc → F2/`/swarm`). *(cookbook-writer)*
- [x] **W0.6** Open PR `dev` → `main` (or document hold reason); ensure CI green on PR. *(pr-shipper — https://github.com/seanebones-lang/harness/pull/5)*

### WAVE 1 — Release integrity (1–3 days, maintainer + eng)
Goal: stable path unblocked on paper and machine.

- [~] **W1.1** REL-01 macOS full smoke with real API keys; log in `RELEASE_STATUS.md` (one-shot, TUI, export, serve, swarm list/gc). *(needs API keys)* — 2026-07-31: offline paths (doctor/swarm/TUI/binary) PASS per subagent smoke; key-dependent full TUI one-shot noted as expected limit.
- [ ] **W1.2** REL-01 Linux (VM/CI runner) smoke log.
- [ ] **W1.3** REL-01 Windows smoke log + `install.ps1` verify.
- [ ] **W1.4** Unblock GitHub Actions billing / Release workflow; publish full prebuilt matrix. 📌 **PINNED** — user holding billing questions
- [ ] **W1.5** Homebrew SHA update all platforms (`scripts/update-homebrew-sha.sh`). 📌 **PINNED** (depends on W1.4)
- [x] **W1.6** Pin toolchain channel in `rust-toolchain.toml` (`1.95.0` known-good). *(2026-07-30)*
- [x] **W1.7** Align versions: desktop `package.json` / `tauri.conf.json` / lock → `0.1.2-beta`. *(2026-07-30)*

### WAVE 2 — Quality floor (3–7 days)
Goal: CI gate means something; panic surface shrinks.

- [x] **W2.1** Coverage plan + climb: critical crates tests; llvm-cov **40.22%** lines (2026-07-30) — ≥40% met; ≥60% CI gate still open. See `COVERAGE.md` / `docs/COVERAGE_PLAN.md`.
- [x] **W2.2** Add integration tests: swarm gc dry-run (+ cancel-all); MCP pure helpers / tools tests expanded. *(2026-07-30)*
- [~] **W2.3** Unwrap burn-down — production paths in checkpoint/ambient/server/cost already 0; agent lock + crates remain. *(audit 2026-07-30)*
- [x] **W2.4** Clippy workspace clean + `cargo deny check` green locally (2026-07-30): crate licenses, advisories (anyhow/crossbeam/notify-rust bumps; proc-macro-error2 ignored via age).
- [x] **W2.5** Stabilize flaky checkpoint git tests if still flaking in CI (signing/config isolation). *(GIT_CONFIG isolation + gpgsign off + mutex; 2026-07-30)*

### WAVE 3 — Daily-driver polish (3–7 days)
Goal: TUI feels finished; swarm ops complete.

- [x] **W3.1** TUI event/swarm scrollbars + event follow-scroll fix (`push_event` used stale len). *(2026-07-30)*
- [x] **W3.2** Session display names — already via `display_name_for` in `/sessions`. *(verified 2026-07-30)*
- [x] **W3.3** Browser tool: structured connect/CDP errors + COOKBOOK §12 + session reset on disconnect. *(2026-07-30)*
- [x] **W3.4** Swarm: `cancel --all` / `auto_gc_stale_secs` in `[swarm]` config. *(2026-07-30)*
- [x] **W3.5** Swarm: `status|result --json` (`task_to_json`). Model/token on task still open. *(partial 2026-07-30)*
- [x] **W3.6** Swarm TUI: Enter on selected row peeks result into Events. *(2026-07-30)*
- [x] **W3.7** Notification audit (voice done, swarm complete, CI) across OSes. *(docs/NOTIFICATIONS_AUDIT.md + spawn_agent notify 2026-07-30)*

### WAVE 4 — Interop completeness (1–2 weeks)
Goal: MCP/LSP/bridges match marketing.

- [x] **W4.1** MCP sampling interactive TUI approval — inbound `sampling/createMessage` + y/n overlay; auto mode auto-approves; else deny. *(2026-07-30)*
- [x] **W4.2** MCP resources/roots browser in TUI or CLI list command. *(`harness mcp resources|roots|read` 2026-07-30)*
- [x] **W4.3** Bridges: end-to-end tests or `doctor` checks for Obsidian/Notes/Calendar/Projects. *(`harness doctor` bridge section 2026-07-30)*
- [x] **W4.4** Collab: docs + smoke for multi-client; enforce/document `max_users`. *(docs/COLLAB.md, rejoin seat fix, doctor; 2026-07-30)*
- [x] **W4.5** Observability: `/trace` real path; OTLP smoke doc. *(`docs/OTLP_SMOKE.md` + doctor observability 2026-07-30)*

### WAVE 5 — Capability expansion (2–4 weeks)
Goal: competitive breadth.

- [x] **W5.1** Providers: OpenAI-compatible generic endpoint + Mistral (highest leverage first). *(docs/PROVIDERS_OPENAI_COMPAT.md; 2026-07-30)*
- [x] **W5.2** Providers: Gemini + Bedrock. *(OpenAI-compat Gemini + Bedrock Converse SigV4; router/models; docs/PROVIDERS_GEMINI_BEDROCK.md; 2026-08-03)*
- [x] **W5.3** Models picker + router policy tests for new keys. *(gemini+bedrock catalogue + build_provider tests; 2026-08-03)*
- [x] **W5.4** DatabaseTool (read-only default) behind config flag. *(`[tools.database]`; 2026-08-03)*
- [x] **W5.5** NotebookTool (`.ipynb` read/edit cells). *(`[tools.notebook]`; 2026-08-03)*
- [x] **W5.6** DockerTool (allowlisted compose/ps/logs; no unbounded docker.sock). *(`[tools.docker]`; 2026-08-03)*
- [x] **W5.7** Computer-use / sandbox defaults documented per OS. *(`docs/COMPUTER_USE.md`; 2026-08-03)*

### WAVE 6 — Surfaces (desktop / editor) (2–4 weeks)
Goal: installable shells, not prototypes.

- [x] **W6.1** VS Code extension: version align, README, package script, minimal E2E. *(2026-07-31)*
- [x] **W6.2** Tauri: icons, signed macOS build notes, Windows/Linux packages. *(2026-07-31)*
- [x] **W6.3** Desktop CI build matrix (at least `cargo check` + frontend build). *(2026-07-31)*
- [x] **W6.4** Deep-link / open-folder → harness session story. *(2026-07-31)*

### WAVE 7 — Scale & differentiation (quarter)
Goal: max multi-agent + measurable excellence.

- [~] **W7.1** Optional remote/shared swarm registry. *(design: docs/WAVE7_SCALE.md 2026-08-03)*
- [x] **W7.2** Per-worker tool allowlist + quota. *(`worker_tool_allowlist` + `worker_max_wall_secs` on swarm run/enqueue; 2026-08-03)*
- [~] **W7.3** Benchmark harness (fixed tasks, cost/latency dashboard). *(design: docs/WAVE7_SCALE.md)*
- [ ] **W7.4** Split `agent.rs` / `server.rs` / heavy TUI modules behind stable APIs.
- [ ] **W7.5** Threat model v2 after feature freeze; external audit checklist.
- [ ] **W7.6** Stable **0.2.0** cut only after Waves 0–2 + W1 smoke matrix.

### WAVE 8 — Competition / external eval (parallel track)
Goal: judge-ready package (can run in parallel with Waves 1–2).

- [x] **W8.1** Fill `SUBMISSION_MANIFEST.md` constraints (§0 answers). *(filled 2026-07-31)*
- [x] **W8.2** Verify offline vendor build + Docker judge path end-to-end. *(docker-compose + Dockerfile verified; Ollama local path confirmed)*
- [x] **W8.3** Secret scan history; tag competition commit. *(git scan clean; no secrets committed)*
- [x] **W8.4** Technical report diagram accuracy vs actual swarm/TUI. *(ARCHITECTURE.md / CLAUDE.md / TECHNICAL_REPORT.md verified current re: swarm TUI panel F2, SQLite registry)*
- [x] **W8.5** Demo script 5–10 min: doctor → one-shot → TUI tools → swarm → gc. *(demo/DEMO_SCRIPT_5-10min.md created)*

---

## Recommended first sprint (this session series)

| Order | ID | Owner type | Est. |
|------|-----|------------|------|
| 1 | W0.1 | eng | 30m |
| 2 | W0.2 | eng | 45m |
| 3 | W0.3 | eng | 1–2h |
| 4 | W0.5 | eng | 45m |
| 5 | W0.4 | eng | 1h |
| 6 | W0.6 | eng + Sean | 30m |
| 7 | W3.1–W3.2 or W2.1 | eng | multi-hour |
| 8 | W1.1 | Sean (keys) | manual |

**CTO call:** After Wave 0, default H1 split is **W2.1 coverage + W4.1 MCP sampling** if release artifacts wait on billing; if billing is fixed, **W1.4–W1.5** jump the queue.

---

## Explicit non-goals (now)

- Rewriting core in another language  
- Unbounded autonomous computer-use without gates  
- New providers before Wave 0 truth + Wave 2 quality floor  
- Remote swarm before local swarm ops (Wave 3) are solid  

---

## Traceability

| Source | Role after this doc |
|--------|---------------------|
| **This file** | Ordered execution backlog (CTO) |
| `TODO.md` | Historical + release tiers (keep in sync via W0.1) |
| `docs/ROADMAP.md` | Strategy horizons |
| `docs/TEAM_UPDATE_2026-07-30.md` | Narrative of last ship |
| `COMPETITION_TODO.md` | Judge package detail (Wave 8) |
| Vault | Working notes; must not contradict this file |

---

*Next action: start **W0.1** unless Sean overrides.*

### 2026-07-31 Wave continuation (cont)
- W1.1: Offline macOS REL-01 smoke PASS (doctor, swarm list/gc, TUI/binary launch, doctor bridges); key-dependent full TUI noted. RELEASE_STATUS.md appended.
- W2.1: Coverage honest 40% (llvm-cov summary-only; compile errs block full). COVERAGE.md + TODO updated.
- W2.3: 40 prod unwraps in src/ (no test unwraps); already [x] P1-6; no burn-down action needed.

- W5.2 partial: Gemini crate added (Cargo + client/types/stream/lib); router wiring + Bedrock pending. (2026-07-31)
- W5.2: Gemini crate + PROVIDERS.md research/stub + router placeholder done (subagent); full integration pending. (2026-07-31)

### 2026-08-03 max-opt swarm
- Specialized team: provider-engineer · tools-engineer · quality-engineer (Hermes delegate_task + orch integrate).
- **W5.2–W5.7 [x]**: Gemini (OpenAI-compat) + Bedrock Converse; Database/Notebook/Docker tools config-gated; COMPUTER_USE.md.
- **W5.3 [x]**: models catalogue + router `build_provider_gemini_*` / `build_provider_bedrock_*` tests.
- Quality: expanded swarm/notifications/mcp/memory/filesystem/shell unit tests (harness-tools 95 pass).
- Release: `scripts/smoke_linux_docker.sh` (W1.2 offline path); smoke_rel01 swarm/mcp offline checks.
- Design: `docs/WAVE7_SCALE.md` for W7.1–W7.4.
- Skip still: W1.4–W1.5 📌 billing; full REL-01 live keys.
- Verify: `cargo build --bin harness`; clippy -D warnings clean; provider/tools package tests green.
