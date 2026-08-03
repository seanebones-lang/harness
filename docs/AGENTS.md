# Specialized Agents — harness max-opt swarm

**Orchestrator:** Hermes (this session)  
**Project:** `/Users/nexteleven/Desktop/harness rework`  
**Backlog:** `docs/CTO_BACKLOG.md`  
**Branch:** `dev`  
**Concurrency:** max 3 implementers (Hermes delegation cap)  
**Mode:** loop until eng-owned open work is exhausted (skip 📌 PINNED billing W1.4–W1.5; skip live-key-only smoke)

## Roster (full-stack max-opt)

| Agent ID | Wave tasks | Specialty | Owns (non-overlapping) |
|----------|------------|-----------|------------------------|
| `provider-engineer` | W5.2, W5.3 | Gemini + Bedrock + router/models | `crates/harness-provider-gemini/**`, `crates/harness-provider-bedrock/**`, `crates/harness-provider-openai/src/**` (gemini/bedrock helpers only), `crates/harness-provider-router/src/lib.rs`, root `Cargo.toml` workspace members only for new providers, `src/cli/commands/models.rs`, `docs/PROVIDERS*.md`, `config/default.toml` provider tables |
| `tools-engineer` | W5.4–W5.7 | Database / Notebook / Docker tools + computer-use docs | `crates/harness-tools/src/tools/{database,notebook,docker}.rs`, `crates/harness-tools/src/tools/mod.rs`, `crates/harness-tools/src/policy.rs`, `crates/harness-tools/src/lib.rs` (re-exports only if needed), `src/cli/wiring.rs` registration + config gates, `src/config.rs` tool flags if required, `config/default.toml` tool sections, `docs/COOKBOOK.md` tool sections, `docs/COMPUTER_USE.md` (W5.7) |
| `quality-engineer` | W2.1 residual, W2.3 crates residual | Coverage climb + pure unit tests | `src/swarm.rs` tests only, `src/notifications.rs` tests only, `crates/harness-mcp/**` tests, `crates/harness-memory/**` tests, `crates/harness-tools/src/tools/{filesystem,apply_patch,shell,swarm_tool}.rs` test modules only (no production logic unless broken), `COVERAGE.md`, `docs/COVERAGE_PLAN.md` |
| `scale-engineer` | W7.1–W7.3 (batch 2) | Swarm scale + benchmarks | `src/swarm.rs` prod (after quality tests land), `docs/BENCHMARKS.md`, `benches/**`, `demo/**` |
| `refactor-surgeon` | W7.4 (batch 2) | Split god files | `src/agent.rs` split only after batch 1 green; coordinate with orch |
| `release-smoker` | W1.1–W1.3 offline | Offline REL smoke logs | `docs/RELEASE_STATUS.md`, docker Linux smoke script only |
| `truth-reconciler` | docs | Backlog honesty | `docs/CTO_BACKLOG.md`, `TODO.md` checkboxes (orch) |
| `pr-shipper` | ship | Commit/push/PR | git/gh (orch only) |

## Rules
1. One agent owns its file set; no two implementers edit the same path in parallel.
2. Implement → self-verify (build/test where code) → report paths + commands run.
3. Orchestrator integrates, runs full `cargo test --bin harness` + package tests, commits, pushes.
4. Do not commit secrets. Do not force-push `main`.
5. Children: **do not commit**. Quote path: `cd "/Users/nexteleven/Desktop/harness rework"`. Use `./target/debug/harness` after build, never PATH `harness`.
6. Entry ritual: `pwd && ls -la && git status && ls crates/`.

## Batch 1 (NOW) — parallel ≤3
- `provider-engineer` · `tools-engineer` · `quality-engineer`

## Batch 2 — after integrate
- `scale-engineer` · residual provider/tools polish · `release-smoker` offline Linux via Docker · truth + ship

## Swarm task IDs
- `sw09dc80fa` provider-engineer W5.2
- `swaa5d9787` tools-engineer W5.4–5.6
- `sw7542b641` quality-engineer W2 coverage

## Skip forever this loop (unless Sean unpins)
- W1.4 / W1.5 📌 billing + Homebrew SHA
- Full REL-01 live TUI one-shot that requires API keys (offline subset OK)
