# harness — Roadmap to Maximum Capability & Compatibility

**Last updated:** 2026-07-30  
**Status base:** public beta GO · stable blocked (REL-01 + prebuilt matrix)  
**Companion:** [`TEAM_UPDATE_2026-07-30.md`](TEAM_UPDATE_2026-07-30.md)

This roadmap is the forward plan for making harness the strongest multi-provider, multi-agent coding system we can ship: **capable**, **compatible**, **safe**, and **operable**.

---

## North star

A single binary (plus optional desktop/editor shells) that:

1. Talks to **every major model backend** with one tool loop.  
2. Runs **parallel agents** with durable tracking, cleanup, and observability.  
3. Works the same on **macOS, Linux, Windows** (install, TUI, headless, CI).  
4. Plugs into **MCP, LSP, browser, voice, and IDE** without special snowflakes.  
5. Stays **fast, low-memory, and auditable** (Rust, tests, threat model).

---

## Compatibility matrix (target)

| Surface | Today | Target |
|---------|--------|--------|
| OS install | macOS arm64 strongest; others partial | Full prebuilts + Homebrew + install scripts all OS |
| Providers | Anthropic, xAI, OpenAI, Ollama, MLX | + Mistral, Gemini, Bedrock (and OpenAI-compatible catch-all) |
| Editors | VS Code extension (early) | Ship-quality VS Code + optional JetBrains later |
| Desktop | Tauri app (early) | Signed macOS/Windows/Linux packages |
| Protocols | MCP client, LSP client, CDP browser | Full MCP sampling UX; robust reconnect; resource browser |
| Multi-agent | Local SQLite swarm + TUI panel + GC | Optional remote workers / shared registry; quotas |
| Headless | CLI, `serve`, swarm, doctor | Stable OpenAPI; auth modes; health for k8s |
| Local models | Ollama + MLX | Documented GPU/CPU paths; embedding defaults |

---

## Capability pillars

### A. Agent core
- Reliable tool loop, plan mode, checkpoints, memory (project + vector).  
- Structured output / schemas; cost + budget guards.  
- Self-dev mode remains opt-in and safe.

### B. Multi-agent (swarm)
- **Done (2026-07-30):** rich CLI, GC, TUI panel, aliases, demo fix.  
- Next: per-task model/tool allowlists; result aggregation UI; cancel-all; metrics export; optional process isolation.

### C. Tools & world interface
- Shell, git, gh, filesystem, search, browser, computer-use, voice.  
- Next: DatabaseTool, NotebookTool, DockerTool; clearer browser errors; sandbox defaults per OS.

### D. Interop
- MCP sampling interactive approval in TUI.  
- Bridges (Obsidian, Notes, Calendar, GitHub Projects) production-hardened.  
- Collab sessions fully documented and tested.

### E. Quality & release
- Coverage ≥ 60% line gate (CI).  
- REL-01 manual smoke green on macOS + Linux + Windows.  
- Clippy/fmt/deny always green; unwrap debt burn-down.  
- Release workflow unblocked (billing) → full artifact matrix.

---

## Horizons

### H0 — Stabilize the line (this week)

| ID | Item | Why | Done when |
|----|------|-----|-----------|
| H0-1 | PR / merge `dev` swarm work to `main` | Ship today’s gains | CI green on PR |
| H0-2 | REL-01 macOS full pass with API keys | Unblocks stable narrative | Logged in RELEASE_STATUS |
| H0-3 | COOKBOOK swarm section (run/list/status/gc/TUI) | Users discover GC/panel | ≥1 worked example |
| H0-4 | `cargo test --all` + clippy on CI for `dev` | No regressions | Green checks |
| H0-5 | Decide H1 priority (coverage vs MCP UX vs TUI polish) | Focus | Written in TODO |

### H1 — Operability & polish (2–4 weeks)

| ID | Item | Capability | Compatibility |
|----|------|------------|---------------|
| H1-1 | Coverage ≥ 60% (auth, bridges, MCP, browser, swarm edge) | Trust | CI gate real |
| H1-2 | TUI scrollbar + follow-scroll; session names | Daily driver | All TUI platforms |
| H1-3 | MCP sampling interactive approval | Interop | Any MCP server |
| H1-4 | Browser tool error surfacing | Reliability | CDP Chrome/Chromium docs |
| H1-5 | Notification kinds polish | UX | macOS/Linux/Windows notify |
| H1-6 | Unwrap burn-down on hot paths | Safety | — |
| H1-7 | Linux REL-01 + prebuilt if CI allows | — | Linux parity |

### H2 — Expand the surface (1–2 months)

| ID | Item | Notes |
|----|------|-------|
| H2-1 | Providers: Mistral, Gemini, Bedrock | Router + env keys + models picker |
| H2-2 | VS Code extension packaging | Marketplace or sideload docs |
| H2-3 | Tauri desktop icons + Windows/Linux packages | Auto-update optional |
| H2-4 | Database / Notebook / Docker tools | Behind config flags |
| H2-5 | Swarm: model override per worker, cancel-all, export JSON | Power users |
| H2-6 | Windows REL-01 + install.ps1 verification | Compatibility |
| H2-7 | i18n expansion beyond partial ES manual | Global users |

### H3 — Maximum system (quarter)

| ID | Item | Notes |
|----|------|-------|
| H3-1 | Shared/remote swarm registry (optional) | Team multi-machine |
| H3-2 | Benchmark harness (SWE-bench-style tracking) | Marketing + regression |
| H3-3 | Formal plugin/extension API freeze | Ecosystem |
| H3-4 | Collab multi-user sessions production | Server mode |
| H3-5 | Security audit refresh post-feature freeze | Threat model v2 |
| H3-6 | Stable 0.2.0 cut | Only after H0–H1 release gates |

---

## Swarm-specific roadmap (post-2026-07-30)

| Priority | Enhancement | Rationale |
|----------|-------------|-----------|
| P1 | `swarm gc --failed-only` / `swarm clean done` ergonomics | Operator muscle memory |
| P1 | Aggregate result view when `--count N` finishes | Parallel review |
| P2 | Persist worker model + token cost on TaskEntry | Cost attribution |
| P2 | TUI select row → show full result pane | No CLI hop |
| P2 | Auto-gc on startup (config: `swarm.auto_gc_stale_secs`) | Prevent silent orphans |
| P3 | Namespace/project-scoped swarm DBs | Multi-repo machines |
| P3 | Remote worker pool | Max multi-agent scale |

---

## Compatibility principles (non-negotiable)

1. **No silent platform gaps** — if a feature is macOS-only, document it and fail clearly elsewhere.  
2. **Config matches behavior** — `config/default.toml` comments must match code (swarm, collab, bridges).  
3. **Demos match flags** — scripts and COOKBOOK always tested against current clap CLI.  
4. **Keys never required for unit tests** — integration/smoke may need keys; unit/CI default path does not.  
5. **Security before glitter** — confirm gates, allowlists, path validation stay ahead of new tools.

---

## Success metrics

| Metric | Beta (now) | Stable target | Max capability |
|--------|------------|---------------|----------------|
| Platforms with smoke log | 1 partial | 3 | 3 + CI artifacts |
| Line coverage | ~39% | ≥60% | ≥70% critical crates |
| Provider count (first-class) | 5 | 5 | 8+ |
| Swarm operability | CLI+TUI+GC | + auto-gc + costs | + remote |
| Time-to-first-success (new user) | docs-dependent | <10 min install+doctor | <5 min wizard |
| P0 security open | 0 | 0 | 0 |

---

## What we will not chase (for now)

- Replacing the Rust core with a scripting runtime.  
- Locked single-vendor cloud.  
- Unbounded autonomous computer-use without confirm gates.  
- Feature sprawl that breaks REL-01 or clippy gates.

---

## How to use this doc

- **Weekly:** pick 1–3 H0/H1 IDs; tick in `TODO.md`.  
- **After each ship:** append a line to `RELEASE_STATUS.md`.  
- **Team sync:** link this roadmap + latest `TEAM_UPDATE_*.md`.

---

*Roadmap is living; prefer PRs that update this file when priorities change.*
