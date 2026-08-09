# Release notes — v1.3.0

**Date:** 2026-08-09  
**Branch:** `main`  
**Tag:** `v1.3.0`  
**Visibility:** public GitHub repository = **proof-of-concept source visibility only**

## License (read this first)

**Proprietary — NextEleven LLC.** See root [`LICENSE`](../LICENSE).

- **Not MIT**
- **Not open source**
- **Not a grant** to use, copy, modify, redistribute, or create derivatives
- Public hosting is for evaluation / proof-of-concept demonstration of capability

Licensing: `legal@nexteleven.com`

## Headline

NextEleven Harness **1.3.0** — multi-provider Rust coding agent (TUI, swarm, MCP, serve) with measured coverage past the CI 60% line gate and proprietary packaging cleaned for a public POC tree.

## Gates (local ship day)

| Gate | Result |
|------|--------|
| Version | **1.3.0** workspace-wide |
| `cargo test --bin harness` | **363** pass |
| `cargo test -p harness-tools` | **179** pass |
| Coverage (`COVERAGE.md`) | **61.65%** lines · CI ≥60% **met** |
| Clippy `-D warnings` (bin) | clean on ship commits |
| Product license labels | proprietary / UNLICENSED (Dockerfile, Homebrew, VS Code, CONTRIBUTING) |

## Notable since 0.1.2-beta

- Providers: Gemini + Bedrock; router catalogue
- Tools: database / notebook / docker (config-gated, default off)
- Swarm: worker model split, allowlist, wall timeout, HTTP registry cutover, cancel-all, `--json`
- TUI: single-panel Hermes-style transcript
- Quality: Swarm-50/51 coverage climb; trust path-inject; wiring pure edges
- Docs: honest badges; public-repo hardening; machine-path scrub

## Install (from source)

```bash
git clone https://github.com/seanebones-lang/harness.git
cd harness
git checkout v1.3.0   # or main
cargo build --profile release-lto
./target/release-lto/harness --version   # expect 1.3.0
```

Prebuilt multi-arch matrix may still be incomplete (GitHub Actions billing 📌). Prefer source build for POC.

## Upgrade notes

- Config stays under `~/.harness/config.toml`
- No automatic MIT relicensing — still proprietary
- Rotate any API keys that ever lived in local `.envrc` (never commit keys)

## Links

- [`CHANGELOG.md`](../CHANGELOG.md)
- [`docs/RELEASE_STATUS.md`](RELEASE_STATUS.md)
- [`SECURITY.md`](../SECURITY.md)
- [`COVERAGE.md`](../COVERAGE.md)
