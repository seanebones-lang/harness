# NextEleven Harness — Open Tasks

**New here?** Start with [`CONTRIBUTING.md`](CONTRIBUTING.md).

**CTO ordered backlog (execution order):** [`docs/CTO_BACKLOG.md`](docs/CTO_BACKLOG.md) ← **use this first**.

Canonical user docs: [`README.md`](README.md). Developer detail: [`CLAUDE.md`](CLAUDE.md), [`config/default.toml`](config/default.toml).

Release readiness: [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) · latest verdict: [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md) · roadmap: [`docs/ROADMAP.md`](docs/ROADMAP.md) · team: [`docs/TEAM_UPDATE_2026-08-03.md`](docs/TEAM_UPDATE_2026-08-03.md)

**Ship branch:** `main` only.  
**Version:** **1.3.0** (public POC).  
**License:** Proprietary NextEleven LLC — **not MIT / not open source**.

---

## Public beta / POC (current)

**Verdict:** **GO** for public POC visibility. **Stable** blocked on REL-01 full OS matrix + prebuilt billing (W1.4–W1.5 📌).

| Gate | Status |
|------|--------|
| Version | **1.3.0** |
| `cargo test --bin harness` | **376** pass (2026-08-24 provider-neutral release gate) |
| `cargo test -p harness-tools` | **179** pass (Swarm-51) |
| Clippy `-D warnings` (bin) | green on ship commits |
| Coverage measured | **61.65%** lines — [`COVERAGE.md`](COVERAGE.md); CI ≥60% **met** |
| License | Proprietary NextEleven LLC ([`LICENSE`](LICENSE)) — public = POC only |

### Tier 0 — Beta shipped

| Task | Status |
|------|--------|
| Public repo, threat model, install docs | [x] |
| CI (Ubuntu, macOS, Windows) | [x] |
| README + CTO backlog | [x] 2026-08-03 docs refresh · Swarm-51 honesty 2026-08-09 |
| Public announcement | [ ] maintainer |

### Tier 1 — Before “stable”

| ID | Task | Status |
|----|------|--------|
| REL-01 | Manual smoke §3 (macOS, Linux, Windows) | [~] offline helpers exist; live keys / full matrix open — W1.1–W1.3 |
| W1.4 | GitHub Actions billing / full Release prebuilts | [ ] 📌 PINNED |
| W1.5 | Homebrew SHA all platforms | [ ] 📌 PINNED (needs W1.4) |
| REL-02 | Tag + verify prebuilts | [~] macOS arm64 history only |
| Stable release label | Publish a supported stable cut only after the full smoke and artifact gates; choose the version at release time | [ ] |

### Tier 2 — Shipped polish (recent)

| Task | Status |
|------|--------|
| Swarm CLI + TUI + GC + worker gates | [x] |
| MCP sampling TUI + resources CLI | [x] |
| Coverage honesty + climb | [x] **61.65%** Swarm-51; CI 60% met |
| Gemini + Bedrock providers | [x] W5.2 |
| Database / Notebook / Docker tools | [x] W5.4–W5.6 config-gated |
| Computer-use docs | [x] W5.7 |
| Offline `harness bench` pack | [x] W7.3 |
| Threat model v2 | [x] W7.5 |
| agent/ + server/ module splits | [x] W7.4 |
| VS Code + Tauri packaging waves | [x] W6 |
| User-owned provider/model routing | [x] W5.8 — exact order, scoped route CLI, 18 built-ins + custom compatible endpoints |

### Tier 3 — Still open eng

| Task | Status |
|------|--------|
| Remote swarm HTTP client + public cutover | [x] W7.1 client 2026-08-04 · cutover 2026-08-05 |
| Coverage → 60% CI gate | [x] measured **61.65%** 2026-08-09 Swarm-51 |
| Residual coverage polish | [~] tui/mod · wiring/driver/input I/O — optional |
| Live provider cost/latency bench | [ ] optional |
| Community channel / announce | [ ] maintainer |

---

## Release checklist

- [x] test / clippy on ship commits (re-run before tag)
- [x] CI coverage gate measured met (60%)
- [ ] REL-01 full matrix — CTO W1.1–W1.3
- [ ] Un-pin billing for multi-arch prebuilts

See [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) §3 · [`docs/CTO_BACKLOG.md`](docs/CTO_BACKLOG.md).
