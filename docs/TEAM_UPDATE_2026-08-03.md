# Team update — 2026-08-03

> **Historical snapshot** (max-opt day). **Current gates:** [`docs/RELEASE_STATUS.md`](RELEASE_STATUS.md) · [`COVERAGE.md`](../COVERAGE.md) · [`TODO.md`](../TODO.md)  
> As of 2026-08-24: bin tests **376**, router tests **15**, tools **179**, coverage **61.65%** at its last measurement (CI 60% **met**), exact user-owned provider/model routing, tip on `main`.

## Headline

Max-opt swarm + docs refresh. **`dev` folded into `main`**; remote/local **`dev` deleted**. Ship branch is **`main` only**.

## Product

| Area | Shipped (as of 2026-08-03) |
|------|---------|
| Providers | Gemini (OpenAI-compat) + Bedrock Converse; Mistral/openai-compat already in |
| Tools | `database` / `notebook` / `docker` config-gated default off |
| Swarm | worker allowlist + wall timeout; model on task JSON; registry trait + selection |
| Quality (then) | ~44.67% llvm-cov; 116 binary tests — **superseded**; see header |
| Bench | `harness bench` offline pack (`demo/bench_tasks`) |
| Structure | `src/agent/*` and `src/server/*` module splits |
| Security docs | Threat model v2 + audit checklist |
| License | Proprietary NextEleven LLC notice on `main` |

## Verify (local)

```bash
cargo test --bin harness          # live count — see RELEASE_STATUS / TODO
cargo clippy -p harness --bin harness -- -D warnings
./target/debug/harness bench
./target/debug/harness models
./target/debug/harness doctor
```

## Docs updated this day

- Full [`README.md`](../README.md) rewrite (CLI, providers, gated tools, honest coverage/license/branch)
- [`CLAUDE.md`](../CLAUDE.md), [`TODO.md`](../TODO.md), [`docs/SHORTCUTS.md`](SHORTCUTS.md), this note, CTO header

## Still open (updated 2026-08-09)

- REL-01 full OS live smoke (offline scripts exist; Docker Linux smoke when daemon up)
- W1.4–W1.5 📌 billing / Homebrew multi-arch
- Coverage → 60% CI gate — **closed / met** (61.65%)
- Remote swarm HTTP client + public cutover — **closed** (W7.1)
- Supported stable cut after the manual smoke and artifact gates; choose the version at release time

## Process

- Prefer `./target/debug/harness` over PATH installs for new flags
- Path has a space if cloned as `harness rework` — quote `cd`
- Coverage badge must track [`COVERAGE.md`](../COVERAGE.md), never invent 60%
