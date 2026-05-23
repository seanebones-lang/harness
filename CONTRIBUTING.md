# Contributing to Harness

Thank you for your interest in contributing. Harness is a Rust coding agent — multi-provider, terminal-first, and built to be hacked on.

---

## Quick orientation

| Doc | What it covers |
|-----|---------------|
| [`README.md`](README.md) | Install, run, daily workflow |
| [`TODO.md`](TODO.md) | **Open backlog (severity-ranked) + roadmap** |
| [`docs/PEER_REVIEW_AUDIT.md`](docs/PEER_REVIEW_AUDIT.md) | Security audit + what was fixed May 2026 |
| [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md) | HTTP/daemon/tool trust boundaries |
| [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md) | Latest go/no-go verification |
| [`CLAUDE.md`](CLAUDE.md) | Module map, key types, adding providers/tools |
| [`docs/INSTALL.md`](docs/INSTALL.md) | Per-OS install, WSL2, troubleshooting |
| [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) | Release checklist + manual smoke §3 |
| [`docs/BROWSER_CDP.md`](docs/BROWSER_CDP.md) | Chrome DevTools / browser tool |
| [`docs/COOKBOOK.md`](docs/COOKBOOK.md) | Example prompts |
| [`config/default.toml`](config/default.toml) | Annotated config reference |

---

## Current state (May 2026)

- **164 automated tests**, clippy clean, CI on Ubuntu/macOS/Windows
- **Public beta GO** — all P0 security items closed ([audit](docs/PEER_REVIEW_AUDIT.md))
- **Stable blocked** on maintainer manual smoke §3 ([`PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md))

---

## Where to contribute (open work)

Prioritized in [`TODO.md`](TODO.md). Highest impact:

| Priority | Good next tasks |
|----------|-----------------|
| 🔴 Critical | Run manual smoke §3; log in [`RELEASE_STATUS.md`](docs/RELEASE_STATUS.md) |
| 🟠 High | Wire or delete `collab` / `bridges` / `diff_review`; MCP allowlist; HTTP rate limits |
| 🟡 Medium | Swarm cancel/wait; proptest MCP/LSP; coverage 70%; VS Code Windows E2E |
| 🟢 Low | CDP screenshots; Spanish manual; Tauri cross-platform packaging |
| ⚪ Optional | `DatabaseTool`, `NotebookTool`, `DockerTool`; new providers |

---

## Setting up

```bash
git clone https://github.com/seanebones-lang/harness.git
cd harness

cargo build
cargo test --all                     # no API keys required
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all

git config core.hooksPath .githooks  # optional: strip AI attribution trailers
```

---

## Recently completed (do not re-implement)

May 2026 peer review remediation + backlog — see [`TODO.md`](TODO.md#recently-completed) and [`PEER_REVIEW_AUDIT.md`](docs/PEER_REVIEW_AUDIT.md).

---

## Submitting changes

1. Fork, branch (`git checkout -b my-feature`).
2. Run gates above.
3. Open a PR with a clear description; link related `TODO.md` IDs if applicable.

See [`CLAUDE.md`](CLAUDE.md) for adding providers and tools.

---

## License

MIT — see [`LICENSE`](LICENSE).
