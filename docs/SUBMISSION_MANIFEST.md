# Submission Manifest

Historical competition / evaluation checklist. **License and gates below match the public `main` tree (2026-08-09).** Older MIT / 218-test language is retired.

## § 0 Constraints (current product)

### Artifact format
- **Format:** Git repository URL (source + Docker)
- **Branch:** `main`
- **Binary:** `cargo build --profile release-lto` → `target/release-lto/harness`
- **Docker:** `docker compose up` or `make docker-build`

### Size
- Source tree stays lean (no large binaries committed)
- Runtime image ~Debian slim + deps; binary ~15–25 MB stripped LTO

### API key policy
- Offline/local demo path: Ollama (no cloud key required)
- Cloud providers optional via env: `ANTHROPIC_API_KEY`, `XAI_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, AWS keys for Bedrock
- Unit/integration gates pass **without** API keys

### Reproducible build
```bash
cargo build --locked --profile release-lto
docker build -t harness:latest .
```
- Toolchain: `rust-toolchain.toml`
- Lockfile: `Cargo.lock` committed
- CI: `.github/workflows/`

### Paper / report
- `docs/TECHNICAL_REPORT.md`

### License
- **Proprietary — NextEleven LLC** ([`LICENSE`](../LICENSE))
- **Not MIT / not open source**
- Licensing contact: `legal@nexteleven.com`

### Contact
- Legal / licensing: `legal@nexteleven.com`
- Product issues: GitHub Issues on this repository (no secrets in issues)

## Quick verification

- [x] `cargo build --profile release-lto`
- [x] `cargo test --bin harness` (live count — see [`TODO.md`](../TODO.md) / [`docs/RELEASE_STATUS.md`](RELEASE_STATUS.md); **363** as of 2026-08-09 cont)
- [x] `cargo test -p harness-tools` (**179** Swarm-51)
- [x] `cargo clippy -p harness --bin harness -- -D warnings`
- [x] Coverage measured **61.65%**; CI ≥60% **met** ([`COVERAGE.md`](../COVERAGE.md))
- [x] License proprietary notice present
- [ ] REL-01 full multi-OS live smoke (offline helpers exist)
- [ ] Multi-arch prebuilts (billing 📌)

## Do not commit

- API keys, `.env`, `.envrc`, tokens, private keys
- Local `~/.harness/` state, swarm DB, session DBs
- Machine-absolute home paths in new notes (use `<repo-root>`)
