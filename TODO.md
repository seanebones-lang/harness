# Harness — Open Tasks

**New here?** Start with [`CONTRIBUTING.md`](CONTRIBUTING.md).

**CTO ordered backlog (execution order):** [`docs/CTO_BACKLOG.md`](docs/CTO_BACKLOG.md) ← **use this first**.

Canonical user docs: [`README.md`](README.md). Developer detail: [`CLAUDE.md`](CLAUDE.md), [`config/default.toml`](config/default.toml).

Release readiness: [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) · latest verdict: [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md) · roadmap: [`docs/ROADMAP.md`](docs/ROADMAP.md) · team: [`docs/TEAM_UPDATE_2026-08-03.md`](docs/TEAM_UPDATE_2026-08-03.md)

**Ship branch:** `main` only.

---

## Public beta (current)

**Verdict:** **GO** for public beta. **Stable** blocked on REL-01 full OS matrix + prebuilt billing (W1.4–W1.5 📌).

| Gate | Status |
|------|--------|
| `cargo test --bin harness` | **116** pass (2026-08-03) |
| Clippy `-D warnings` (bin) | green on ship commits |
| Coverage measured | **44.67%** lines — [`COVERAGE.md`](COVERAGE.md); CI target 60% open |
| License | Proprietary NextEleven LLC ([`LICENSE`](LICENSE)) |

### Tier 0 — Beta shipped

| Task | Status |
|------|--------|
| Public repo, threat model, install docs | [x] |
| CI (Ubuntu, macOS, Windows) | [x] |
| README + CTO backlog | [x] 2026-08-03 docs refresh |
| Public announcement | [ ] maintainer |

### Tier 1 — Before “stable”

| ID | Task | Status |
|----|------|--------|
| REL-01 | Manual smoke §3 (macOS, Linux, Windows) | [~] offline helpers exist; live keys / full matrix open — W1.1–W1.3 |
| W1.4 | GitHub Actions billing / full Release prebuilts | [ ] 📌 PINNED |
| W1.5 | Homebrew SHA all platforms | [ ] 📌 PINNED (needs W1.4) |
| REL-02 | Tag + verify prebuilts | [~] macOS arm64 history only |
| W7.6 | Stable **0.2.0** | [ ] after Waves 0–2 smoke matrix |

### Tier 2 — Shipped polish (recent)

| Task | Status |
|------|--------|
| Swarm CLI + TUI + GC + worker gates | [x] |
| MCP sampling TUI + resources CLI | [x] |
| Coverage honesty + climb ~45% | [x] measured |
| Gemini + Bedrock providers | [x] W5.2 |
| Database / Notebook / Docker tools | [x] W5.4–W5.6 config-gated |
| Computer-use docs | [x] W5.7 |
| Offline `harness bench` pack | [x] W7.3 |
| Threat model v2 | [x] W7.5 |
| agent/ + server/ module splits | [x] W7.4 |
| VS Code + Tauri packaging waves | [x] W6 |

### Tier 3 — Still open eng

| Task | Status |
|------|--------|
| Remote swarm HTTP client (beyond stub) | [~] W7.1 trait+stub |
| Coverage → 60% CI gate | [ ] |
| Live provider cost/latency bench | [ ] optional |
| Community channel / announce | [ ] maintainer |

---

## Release checklist

- [x] test / clippy on ship commits (re-run before tag)
- [ ] REL-01 full matrix — CTO W1.1–W1.3
- [ ] Un-pin billing for multi-arch prebuilts

See [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) §3 · [`docs/CTO_BACKLOG.md`](docs/CTO_BACKLOG.md).
