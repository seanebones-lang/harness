# Submission Manifest

This document answers the competition's § 0 constraints. Fields marked
`[TO BE FILLED BY TEAM]` must be completed before final submission.

## § 0 Competition Constraints

### Artifact Format
- **Required format**: `[TO BE FILLED BY TEAM — e.g., .zip, .tar.gz, Docker image, GitHub repo URL]`
- **Submitted as**: Git repository on branch `claude/trusting-brahmagupta-5qnqc`
- **Binary produced by**: `cargo build --profile release-lto` → `target/release-lto/harness`
- **Docker image**: Built via `docker compose up` or `make docker-build`

### Size Limit
- **Competition size limit**: `[TO BE FILLED BY TEAM — e.g., 500 MB, 1 GB, no limit]`
- **Estimated repo size (source)**: < 5 MB (no large binaries committed)
- **Estimated Docker image size**: ~120 MB runtime layer (Debian bookworm-slim + runtime libs)
- **Compiled binary size**: ~15–25 MB (stripped, LTO)

### API Key Policy
- **Competition API key policy**: `[TO BE FILLED BY TEAM — whether judges supply keys or project must run without]`
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
- **Competition paper requirement**: `[TO BE FILLED BY TEAM — page limit, format, venue]`
- **Our report**: `docs/TECHNICAL_REPORT.md` — `[TO BE FILLED BY TEAM — confirm this file exists and link the rendered PDF if required]`

### License
- **Required license**: `[TO BE FILLED BY TEAM — e.g., Apache-2.0, MIT, or open source required]`
- **Our license**: MIT — see `LICENSE` file in repository root

### Deadline
- **Submission deadline**: `[TO BE FILLED BY TEAM — date and time with timezone]`
- **Our submission date**: 2026-05-25
- **Branch**: `claude/trusting-brahmagupta-5qnqc`
- **Contact**: nextelevenstudios@gmail.com

## Quick Verification Checklist

- [ ] `cargo build --profile release-lto` succeeds from a clean checkout
- [ ] `cargo test --all` passes (218 tests, no API keys needed)
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` clean
- [ ] `docker compose up` starts Ollama + Harness successfully
- [ ] `[TO BE FILLED BY TEAM]` artifact format confirmed with organizers
- [ ] `[TO BE FILLED BY TEAM]` paper/report submitted to correct venue
- [ ] `[TO BE FILLED BY TEAM]` final submission portal entry completed
