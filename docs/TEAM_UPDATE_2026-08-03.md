# Team update — 2026-08-03

## Headline

Max-opt swarm + docs refresh. **`dev` folded into `main`**; remote/local **`dev` deleted**. Ship branch is **`main` only**.

## Product

| Area | Shipped |
|------|---------|
| Providers | Gemini (OpenAI-compat) + Bedrock Converse; Mistral/openai-compat already in |
| Tools | `database` / `notebook` / `docker` config-gated default off |
| Swarm | worker allowlist + wall timeout; model on task JSON; registry trait + selection |
| Quality | ~44.67% llvm-cov measured; 116 binary tests |
| Bench | `harness bench` offline pack (`demo/bench_tasks`) |
| Structure | `src/agent/*` and `src/server/*` module splits |
| Security docs | Threat model v2 + audit checklist |
| License | Proprietary NextEleven LLC notice on `main` |

## Verify (local)

```bash
cargo test --bin harness          # 116 pass
cargo clippy -p harness --bin harness -- -D warnings
./target/debug/harness bench
./target/debug/harness models
./target/debug/harness doctor
```

## Docs updated this day

- Full [`README.md`](../README.md) rewrite (CLI, providers, gated tools, honest coverage/license/branch)
- [`CLAUDE.md`](../CLAUDE.md), [`TODO.md`](../TODO.md), [`docs/SHORTCUTS.md`](SHORTCUTS.md), this note, CTO header

## Still open

- REL-01 full OS live smoke (offline scripts exist)
- W1.4–W1.5 📌 billing / Homebrew multi-arch
- Coverage → 60% CI gate
- Remote swarm HTTP client beyond stub
- Stable 0.2.0 cut

## Process

- Prefer `./target/debug/harness` over PATH installs for new flags
- Path has a space if cloned as `harness rework` — quote `cd`
- Coverage badge must track [`COVERAGE.md`](../COVERAGE.md), never invent 60%
