# harness — Roadmap to Maximum Capability & Compatibility

**Last updated:** 2026-08-24
**Status base:** public beta GO · stable blocked (REL-01 + prebuilt matrix) · ship on **`main`**  
**Companion:** [`TEAM_UPDATE_2026-08-03.md`](TEAM_UPDATE_2026-08-03.md) · ordered work [`CTO_BACKLOG.md`](CTO_BACKLOG.md)

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
| OS install | Scripts + CI all OS; prebuilts partial | Full prebuilts + Homebrew all arch (billing 📌) |
| Providers | Exact user-owned route; 18 built-in names + custom OpenAI-format endpoints | Keep compatibility metadata current; add native adapters only for genuinely different protocols |
| Editors | VS Code + Tauri waves landed | Signed packages polish |
| Protocols | MCP sampling UX + resources CLI; CDP browser | Robust reconnect polish |
| Multi-agent | Local SQLite swarm + allowlist/wall + TUI; remote HTTP registry cutover shipped | Operability, isolation, aggregation, and cost attribution polish |
| Headless | CLI, serve, swarm, doctor, bench | Stable OpenAPI; k8s health |
| Local models | Ollama + MLX | Documented GPU/CPU paths |
| Coverage | 61.65% measured; ≥60% CI gate met | Maintain the gate and deepen high-risk/low-I/O paths |

---

## Capability pillars

### A. Agent core
- Reliable tool loop, plan mode, checkpoints, memory (project + vector).  
- Structured output / schemas; cost + budget guards.  
- Self-dev mode remains opt-in and safe.

### B. Multi-agent (swarm)
- **Done:** rich CLI, GC, TUI panel, aliases, remote-registry cutover, per-worker model/tool allowlists, wall timeouts, cancel-all, and JSON output.
- Next: result aggregation UX, cost attribution, metrics export, and optional process isolation.

### C. Tools & world interface
- Shell, git, gh, filesystem, search, browser, computer-use, voice.  
- Database, notebook, and Docker tools ship behind config gates and default off.
- Next: clearer errors, stronger cross-platform tests, and narrower authority where a tool can mutate external state.

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
| H0-1 | REL-01 manual provider/TUI/browser smoke on each target OS | Unblocks stable narrative | Exact results logged in RELEASE_STATUS |
| H0-2 | Resolve release-artifact billing and rebuild the full prebuilt matrix | Makes install claims reproducible | Every advertised artifact downloaded and verified |
| H0-3 | Exercise setup and route editing on clean macOS/Linux/Windows profiles | Protects the new provider-neutral contract | Setup + show/set/model/add/remove/move/custom smoke recorded |
| H0-4 | Keep public descriptions synchronized with code | Prevents stale provider/default claims | README, manuals, site, and release status agree |
| H0-5 | Cut the next version only after H0-1 through H0-3 | Avoids relabeling `main` as a shipped tag | Versioned notes describe only included commits |

### H1 — Operability & polish (2–4 weeks)

| ID | Item | Capability | Compatibility |
|----|------|------------|---------------|
| H1-1 | Maintain coverage ≥ 60% and deepen auth, bridges, MCP, browser, route, and swarm edges | Trust | CI gate stays green |
| H1-2 | TUI scrollbar + follow-scroll; session names | Daily driver | All TUI platforms |
| H1-3 | MCP sampling and resource interoperability polish | Interop | Representative MCP servers |
| H1-4 | Browser tool error surfacing | Reliability | CDP Chrome/Chromium docs |
| H1-5 | Notification kinds polish | UX | macOS/Linux/Windows notify |
| H1-6 | Unwrap burn-down on hot paths | Safety | — |
| H1-7 | Linux REL-01 + prebuilt if CI allows | — | Linux parity |

### H2 — Expand the surface (1–2 months)

| ID | Item | Notes |
|----|------|-------|
| H2-1 | Native adapters for non-compatible protocols only | Preserve user choice without duplicating compatible clients |
| H2-2 | VS Code extension packaging | Marketplace or sideload docs |
| H2-3 | Tauri desktop icons + Windows/Linux packages | Auto-update optional |
| H2-4 | Database / Notebook / Docker hardening | Existing config-gated tools; improve safety and platform coverage |
| H2-5 | Swarm result aggregation and cost attribution | Existing model override, cancel-all, and JSON paths are shipped |
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
| H3-6 | Supported stable cut | Choose the version only after H0–H1 release gates; do not backslide from the current 1.3.0 POC version |

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
| Line coverage | 61.65% measured | Maintain ≥60% | ≥70% critical crates |
| Provider surface | 18 built-in names + custom compatible endpoints | Clean-route smoke on 3 OSes | Protocol depth without vendor preference |
| Swarm operability | CLI+TUI+GC+remote registry | + auto-gc + costs | + optional worker isolation |
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
