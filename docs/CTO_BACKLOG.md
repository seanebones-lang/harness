# CTO Engineering Backlog — harness

**Date:** 2026-07-30  
**Perspective:** CTO review of codebase + Obsidian vault  
**Branch base:** `dev` @ `f06ee9b`  
**Verdict:** Strong multi-provider agent core; **operability and truthfulness of docs/metrics lag the architecture**. Ship order below maximizes trust → stability → capability → scale.

**How to use:** Work top-to-bottom within each wave. Do not start Wave N+1 items that block Wave N release gates unless explicitly re-prioritized. Check boxes in this file as we execute.

---

## Executive findings (gaps)

### A. Trust & release (business-critical)
| Gap | Evidence | Risk |
|-----|----------|------|
| Stable blocked | REL-01 incomplete; Linux/Windows smoke missing | Cannot claim production-ready |
| Prebuilt matrix partial | macOS arm64 only; CI billing history | Broken install story outside one arch |
| Coverage myth | Badge “60%+”; `COVERAGE.md` ≈ **23%** lines | Credibility hit with judges/users |
| Doc/status drift | `TODO.md` marks scrollbar/session-names **[x]**; vault says not started; chat scrollbar exists but follow-scroll/session UX incomplete | Team executes the wrong work |
| Vault hollow | `Vault/Architecture/`, `Vault/Backlog/` empty; Condition still says `main` | Knowledge system not operational |

### B. Product honesty (UX debt)
| Gap | Evidence | Risk |
|-----|----------|------|
| Slash stubs | `/obsidian` → “Phase E12”; `/trace` → “Phase E7” while features exist | Users think product is vapor |
| COOKBOOK gap | No swarm / gc / F2 worked example | Feature undiscoverable |
| Competition checklist open | Many **[C]** boxes still open despite Dockerfile/etc. | Submission not judge-ready |

### C. Architecture & quality
| Gap | Evidence | Risk |
|-----|----------|------|
| God files | `agent.rs` ~1k, `server.rs` ~1.2k, `tui/render|input|driver` large | Change risk, review cost |
| Unwrap surface | ~130 `src/` + ~200 `crates/` unwrap/expect | Panic paths under poison/IO |
| MCP sampling UX | Auto vs deny only; no interactive TUI approval | MCP servers half-usable in plan mode |
| Version drift | Desktop `package.json` 0.1.0 vs crate 0.1.2-beta; tauri.conf 0.1.0 | Support confusion |
| Toolchain pin | `rust-toolchain.toml` = `stable` not dated channel | Non-reproducible MSRV story |
| Swarm next | No cancel-all, cost-on-task, auto-gc config, result aggregate | Ops ceiling after today’s win |

### D. Compatibility surface
| Gap | Notes |
|-----|--------|
| Windows/Linux release artifacts | Install scripts exist; Release matrix incomplete |
| Provider breadth | No Mistral/Gemini/Bedrock first-class |
| Editor/desktop | VS Code + Tauri early; packaging incomplete |
| i18n | ES manual partial/draft |

---

## Ordered TODO (execute in this sequence)

### WAVE 0 — Truth & alignment (½–1 day)  ← **START HERE**
Goal: one backlog, honest metrics, vault usable, no lying checkboxes.

- [ ] **W0.1** Reconcile `TODO.md` P2-1/P2-2/P2-3 with reality (scrollbar partial, session names, notifications). Downgrade false [x] → [~] or [ ] with notes.
- [ ] **W0.2** Fix coverage messaging: badge/README/CLAUDE vs `COVERAGE.md` (23%). Either remeasure and document true % or remove “60%+” badge until gate is real.
- [ ] **W0.3** Wire slash stubs to real implementations or honest “not available” with doc links (`/obsidian`, `/trace` at minimum).
- [ ] **W0.4** Populate vault: `Architecture/Overview.md`, `Backlog/CTO-TODO.md` (link this file), refresh `Status/Current-Condition.md` for `dev`.
- [ ] **W0.5** COOKBOOK: add swarm worked example (run → list → status → gc → F2/`/swarm`).
- [ ] **W0.6** Open PR `dev` → `main` (or document hold reason); ensure CI green on PR.

### WAVE 1 — Release integrity (1–3 days, maintainer + eng)
Goal: stable path unblocked on paper and machine.

- [ ] **W1.1** REL-01 macOS full smoke with real API keys; log in `RELEASE_STATUS.md` (one-shot, TUI, export, serve, swarm list/gc).
- [ ] **W1.2** REL-01 Linux (VM/CI runner) smoke log.
- [ ] **W1.3** REL-01 Windows smoke log + `install.ps1` verify.
- [ ] **W1.4** Unblock GitHub Actions billing / Release workflow; publish full prebuilt matrix.
- [ ] **W1.5** Homebrew SHA update all platforms (`scripts/update-homebrew-sha.sh`).
- [ ] **W1.6** Pin toolchain channel (e.g. `1.85.0` or current known-good) in `rust-toolchain.toml`; verify `cargo test --all` on pin.
- [ ] **W1.7** Align versions: desktop `package.json` / `tauri.conf.json` / workspace `0.1.2-beta`.

### WAVE 2 — Quality floor (3–7 days)
Goal: CI gate means something; panic surface shrinks.

- [ ] **W2.1** Coverage plan: critical crates first (`harness-tools`, `agent`, `swarm`, auth paths, MCP client). Target **≥40%** workspace then **≥60%** on critical crates.
- [ ] **W2.2** Add integration tests: swarm gc dry-run, MCP load failure paths, browser CDP unavailable, daemon auth reject.
- [ ] **W2.3** Unwrap burn-down pass on `src/agent.rs`, `src/server.rs`, `src/swarm.rs`, tools hot paths (no silent `unwrap` on IO/lock).
- [ ] **W2.4** `cargo deny` + clippy workspace clean on CI for `dev` and `main`.
- [ ] **W2.5** Stabilize flaky checkpoint git tests if still flaking in CI (signing/config isolation).

### WAVE 3 — Daily-driver polish (3–7 days)
Goal: TUI feels finished; swarm ops complete.

- [ ] **W3.1** TUI follow-scroll correctness + event/swarm scrollbars parity (finish real P2-1).
- [ ] **W3.2** Session display names in `/sessions` and status (finish real P2-2).
- [ ] **W3.3** Browser tool: structured errors + COOKBOOK troubleshooting link.
- [ ] **W3.4** Swarm: `cancel --all` / `gc` defaults in config (`[swarm] auto_gc_stale_secs`).
- [ ] **W3.5** Swarm: persist worker model + token summary on task; `swarm result --json`.
- [ ] **W3.6** Swarm TUI: select row → full result; aggregate multi-count runs.
- [ ] **W3.7** Notification audit (voice done, swarm complete, CI) across OSes.

### WAVE 4 — Interop completeness (1–2 weeks)
Goal: MCP/LSP/bridges match marketing.

- [ ] **W4.1** MCP sampling interactive TUI approval (default deny; y/n with preview).
- [ ] **W4.2** MCP resources/roots browser in TUI or CLI list command.
- [ ] **W4.3** Bridges: end-to-end tests or `doctor` checks for Obsidian/Notes/Calendar/Projects.
- [ ] **W4.4** Collab: docs + smoke for multi-client; enforce/document `max_users`.
- [ ] **W4.5** Observability: `/trace` real path; OTLP smoke doc.

### WAVE 5 — Capability expansion (2–4 weeks)
Goal: competitive breadth.

- [ ] **W5.1** Providers: OpenAI-compatible generic endpoint + Mistral (highest leverage first).
- [ ] **W5.2** Providers: Gemini + Bedrock.
- [ ] **W5.3** Models picker + router policy tests for new keys.
- [ ] **W5.4** DatabaseTool (read-only default) behind config flag.
- [ ] **W5.5** NotebookTool (`.ipynb` read/edit cells).
- [ ] **W5.6** DockerTool (allowlisted compose/ps/logs; no unbounded docker.sock).
- [ ] **W5.7** Computer-use / sandbox defaults documented per OS.

### WAVE 6 — Surfaces (desktop / editor) (2–4 weeks)
Goal: installable shells, not prototypes.

- [ ] **W6.1** VS Code extension: version align, README, package script, minimal E2E.
- [ ] **W6.2** Tauri: icons, signed macOS build notes, Windows/Linux packages.
- [ ] **W6.3** Desktop CI build matrix (at least `cargo check` + frontend build).
- [ ] **W6.4** Deep-link / open-folder → harness session story.

### WAVE 7 — Scale & differentiation (quarter)
Goal: max multi-agent + measurable excellence.

- [ ] **W7.1** Optional remote/shared swarm registry.
- [ ] **W7.2** Per-worker tool allowlist + quota.
- [ ] **W7.3** Benchmark harness (fixed tasks, cost/latency dashboard).
- [ ] **W7.4** Split `agent.rs` / `server.rs` / heavy TUI modules behind stable APIs.
- [ ] **W7.5** Threat model v2 after feature freeze; external audit checklist.
- [ ] **W7.6** Stable **0.2.0** cut only after Waves 0–2 + W1 smoke matrix.

### WAVE 8 — Competition / external eval (parallel track)
Goal: judge-ready package (can run in parallel with Waves 1–2).

- [ ] **W8.1** Fill `SUBMISSION_MANIFEST.md` constraints (§0 answers).
- [ ] **W8.2** Verify offline vendor build + Docker judge path end-to-end.
- [ ] **W8.3** Secret scan history; tag competition commit.
- [ ] **W8.4** Technical report diagram accuracy vs actual swarm/TUI.
- [ ] **W8.5** Demo script 5–10 min: doctor → one-shot → TUI tools → swarm → gc.

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
