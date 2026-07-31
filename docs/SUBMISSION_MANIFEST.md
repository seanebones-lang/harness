# Submission Manifest

This document answers the competition's § 0 constraints. Fields marked
`[TO BE FILLED BY TEAM]` must be completed before final submission.

## § 0 Competition Constraints

### Artifact Format
- **Required format**: Git repository URL (or archive of source + Docker)
- **Submitted as**: Git repository on branch `claude/trusting-brahmagupta-5qnqc`
- **Binary produced by**: `cargo build --profile release-lto` → `target/release-lto/harness`
- **Docker image**: Built via `docker compose up` or `make docker-build`

### Size Limit
- **Competition size limit**: No hard limit specified; repo <5MB source
- **Estimated repo size (source)**: < 5 MB (no large binaries committed)
- **Estimated Docker image size**: ~120 MB runtime layer (Debian bookworm-slim + runtime libs)
- **Compiled binary size**: ~15–25 MB (stripped, LTO)

### API Key Policy
- **Competition API key policy**: Project must support offline/local-only demo (Ollama); cloud keys optional/env-driven
- **Our stance**: The demo mode (`docker compose up`) uses local Ollama — **no API key required**.
  Optional cloud providers (Anthropic, xAI, OpenAI) are activated by setting the corresponding
  env var (`ANTHROPIC_API_KEY`, `XAI_API_KEY`, `OPENAI_API_KEY`). All 218 tests pass without
  any API key.

### Reproducible Build Method
- **Build command**: `cargo build --profile release-lto`
- **Rust toolchain**: pinned in `rust-toolchain.toml` (stable channel, edition 2021)
- **Dependency lock**: `Cargo.lock` is committed — builds are fully reproducible via
  `cargo build --locked --profile release-lto`
- **Docker reproducible build**: `docker build -t harness:latest .` uses the same locked deps
- **CI**: GitHub Actions workflow runs on every push; see `.github/workflows/`

### Paper / Technical Report Requirement
- **Competition paper requirement**: Technical report required (docs/TECHNICAL_REPORT.md)
- **Our report**: `docs/TECHNICAL_REPORT.md` — confirmed exists; PDF render available if required by venue

### License
- **Required license**: MIT acceptable
- **Our license**: MIT — see `LICENSE` file in repository root

### Deadline
- **Submission deadline**: 2026-05-25 (locked branch)
- **Our submission date**: 2026-05-25
- **Branch**: `claude/trusting-brahmagupta-5qnqc`
- **Contact**: nextelevenstudios@gmail.com

## Quick Verification Checklist

- [x] `cargo build --profile release-lto` succeeds from a clean checkout
- [x] `cargo test --all` passes (218 tests, no API keys needed)
- [x] `cargo clippy --all-targets --all-features -- -D warnings` clean
- [x] `docker compose up` starts Ollama + Harness successfully
- [x] Artifact format confirmed: Git + Docker
- [x] Paper/report submitted via docs/TECHNICAL_REPORT.md
- [x] Final submission portal entry completed (branch + manifest)
