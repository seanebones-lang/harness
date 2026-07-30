# Harness — Open Tasks

**New here?** Start with [`CONTRIBUTING.md`](CONTRIBUTING.md).

**CTO ordered backlog (execution order):** [`docs/CTO_BACKLOG.md`](docs/CTO_BACKLOG.md) ← **use this first**.

Canonical user docs: [`README.md`](README.md). Developer detail: [`CLAUDE.md`](CLAUDE.md), [`config/default.toml`](config/default.toml).

Release readiness: [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) · latest verdict: [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md) · roadmap: [`docs/ROADMAP.md`](docs/ROADMAP.md) · team brief: [`docs/TEAM_UPDATE_2026-07-30.md`](docs/TEAM_UPDATE_2026-07-30.md)

---

## Public beta promotion (May 2026)

**Verdict:** **GO** for public beta now. **Stable** blocked on REL-01 + P2-10 (see Tier 1).

### Tier 0 — Ship beta now

| Task | Status |
|------|--------|
| Public repo, MIT, threat model, install docs | [x] |
| CI (Ubuntu, macOS, Windows) + automated smoke subset | [x] |
| README screenshots + comparison link | [x] |
| Promotion report + draft release notes | [x] |
| Public announcement (HN, X, Discussions) | [ ] maintainer |

### Tier 1 — Before “stable”

| ID | Task | Status |
|----|------|--------|
| REL-01 | Manual smoke §3 (macOS, Linux, Windows) | [~] macOS partial — CTO W1.1–W1.3 |
| P2-10 | Homebrew tap SHA after tag | [~] macOS arm64 only — W1.5 |
| REL-02 | Tag v0.1.2-beta + verify prebuilts | [~] macOS arm64 only |
| REL-03 | Log REL-01 per OS | [x] partial |

### Tier 2 — High-impact polish

| Task | Status |
|------|--------|
| COMPARISON / CONTRIBUTING / labels | [x] |
| Demo GIF/video | [ ] optional |
| Swarm CLI + TUI + GC (2026-07-30) | [x] |
| TUI scrollbar + follow-scroll | [x] CTO W3.1 |
| Session list display names | [x] CTO W3.2 |
| Slash stubs (`/obsidian`, `/trace`) | [x] CTO W0.3 |
| COOKBOOK swarm + MCP examples | [x] W0.5 / W4.2 |
| Coverage honesty (badge vs ~23%) | [x] W0.2; climb W2.1 [~] |
| MCP resources/roots CLI | [x] W4.2 `harness mcp` |
| Notification audit | [x] W3.7 |

### Tier 3 — Growth

| Task | Status |
|------|--------|
| MCP sampling interactive TUI approval | [x] CTO W4.1 |
| DatabaseTool / NotebookTool / DockerTool | [ ] CTO W5.4–W5.6 |
| New providers (Mistral, Gemini, Bedrock) | [ ] CTO W5.1–W5.2 |
| VS Code + Tauri packaging | [ ] CTO W6.* |
| Community channel | [ ] optional |
| Full CTO waves 0–8 | [~] [`docs/CTO_BACKLOG.md`](docs/CTO_BACKLOG.md) |

---

## Historical P0–P2 (May 2026)

Most P0/P1 closed. P2 reconciled 2026-07-30:

| ID | Item | Status |
|----|------|--------|
| P2-1 | Scrollbar + follow-scroll | [x] W3.1 |
| P2-2 | Session names | [x] W3.2 |
| P2-3 | Notification kinds | [x] W3.7 |
| P2-4 | Swarm status | [x] + panel |
| P2-5–P2-6 | Collab max_users; browser Err | [x] |
| P2-7 | Coverage | [~] ~23% — W2.1 plan + tests |
| P2-8–P2-9 | VS Code / desktop CI | [~] W6 |
| P2-10 | Homebrew | [ ] maintainer |

---

## Release checklist

- [x] test / clippy / fmt / release-lto (re-run before tag)
- [ ] REL-01 full matrix — CTO W1.1–W1.3

See [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) §3 · [`docs/CTO_BACKLOG.md`](docs/CTO_BACKLOG.md).
