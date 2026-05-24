# Harness — Open Tasks

**New here?** Start with [`CONTRIBUTING.md`](CONTRIBUTING.md).

Canonical user docs: [`README.md`](README.md). Developer detail: [`CLAUDE.md`](CLAUDE.md), [`config/default.toml`](config/default.toml).

Release readiness: [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) · latest verdict: [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md)

---

## Immediate Action Plan (May 2026 Audit)

Execution order: **P0 → P1 → P2**. **Maintainer-only** items need API keys / manual GUI.

### P0 — Critical

| ID | Item | Status |
|----|------|--------|
| P0-1 | Eliminate `ProviderRouter has no providers` panic | [x] |
| P0-2 | Replace XAI `.unwrap()` on missing API key | [x] |
| P0-3 | Align prebuilt artifact names | [x] |
| P0-4 | Add Windows to release workflow | [x] |
| P0-5 | Reconcile version strings | [x] |
| P0-6 | Delete merge artifacts; gitignore `*.orig`/`*.rej` | [x] |
| P0-7 | Wizard robustness + `harness setup` | [x] |
| P0-8 | Install scripts warn on binary path conflicts | [x] |

### P1 — High priority

| ID | Item | Status |
|----|------|--------|
| P1-1 | Wire MCP inbound requests (sampling) | [x] |
| P1-2 | MCP spawn command allowlist default | [x] |
| P1-3 | Escape AppleScript inputs in calendar bridge | [x] |
| P1-4 | Protect `/api/setup/state` (strip config path) | [x] |
| P1-5 | Constant-time bearer token comparison | [x] |
| P1-6 | Replace production `.unwrap()`/`.expect()` hot paths | [x] |
| P1-7 | Stale default model IDs → `claude-sonnet-4-6` | [x] |
| P1-8 | Router unit tests | [x] |
| P1-9 | Daemon/auth rejection tests | [x] |
| P1-10 | Swarm CLI integration tests (prefix lookup) | [x] |
| P1-11 | README factual fixes | [x] |
| P1-12 | Refresh stale docs | [x] |
| P1-13 | Implement `harness update` subcommand | [x] |
| P1-14 | Add `release-lto` to CI | [x] |

### P2 — Polish

| ID | Item | Status |
|----|------|--------|
| P2-1 | TUI visible scrollbar + follow-scroll | [x] |
| P2-2 | Session list display names (first-message fallback) | [x] |
| P2-3 | Wire notification kinds (voice, swarm wait) | [x] |
| P2-4 | Swarm background status updates | [x] (via existing `spawn_task`) |
| P2-5 | Collab `max_users` enforcement | [x] |
| P2-6 | Browser tool `unreachable!()` → `Err` | [x] |
| P2-7 | Coverage uplift (auth, bridges, swarm prefix tests) | [x] |
| P2-8 | VS Code extension README + packaging notes | [x] |
| P2-9 | Desktop app CI check (Tauri `cargo check`) | [x] |
| P2-10 | Homebrew tap publish | [ ] maintainer (`scripts/update-homebrew-sha.sh`) |

### Maintainer-only

| ID | Item | Status |
|----|------|--------|
| REL-01 | Manual smoke §3 | [ ] (`scripts/smoke_rel01.sh` for automated subset) |

---

## Remaining backlog

- Homebrew tap publish: run `bash scripts/update-homebrew-sha.sh vX.Y.Z` after tagging (P2-10)
- REL-01 manual smoke on target OSes (automated subset: `scripts/smoke_rel01.sh` — rebuild binary first)
- MCP sampling interactive TUI approval (plan/smart currently deny inbound sampling)
- Coverage gate uplift toward 60% on more crates (voice, mlx, lsp client)
- DatabaseTool / NotebookTool / DockerTool
- New providers (Mistral, Gemini, Bedrock)
- Tauri app icons generation (CI check only today)

---

## Release checklist (maintainers)

- [x] `cargo test --all`
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] `cargo fmt --all -- --check`
- [x] `cargo build --profile release-lto` (CI + local)

Manual smoke (REL-01): see [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) §3.
