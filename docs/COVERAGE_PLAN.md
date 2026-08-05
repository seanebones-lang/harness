# Coverage plan (W2.1)

Short uplift plan for workspace line coverage. **Measured baseline** comes from root [`COVERAGE.md`](../COVERAGE.md) — do not invent higher numbers.

| Field | Value |
|-------|--------|
| **Current measured** | **51.98%** lines (12224 / 23516; llvm-cov, 2026-08-05 swarm-50) — near-term ≥40% **met** |
| **CI gate (target)** | ≥ 60% lines via `cargo llvm-cov --fail-under-lines 60` — **not met** |
| **Near-term goal** | Workspace **≥ 40%** ✓, then critical crates **≥ 60%** |
| **Stretch** | Meet CI **≥ 60%** workspace |

## Priority order (critical crates first)

1. **`harness-tools`** — tool input validation, denylist/confirm policy, workspace sandbox, swarm/agent enqueue tools, executor gates. High pure-logic density; no network.
2. **Agent / swarm (`src/swarm.rs`, agent loop helpers)** — status labels, GC, task JSON, counts, format helpers; then agent pure branches.
3. **`harness-mcp`** — config parse, command allowlist, text/content helpers, in-process duplex client tests (already strong — keep expanding pure helpers).
4. **Auth / tokens (`src/auth_token.rs`, trust)** — parsing, expiry, allow/deny without live providers.
5. Providers / TUI / CLI — later: harder I/O and UI surface; prefer extracting pure helpers before heavy integration tests.

## Concrete next test modules

| Module | Status / what to cover next |
|--------|------------------------------|
| `crates/harness-tools/src/tools/filesystem.rs` | **Done (W2):** missing args, line-range clamp, patch uniqueness/dry_run, trim_context, def names (~99% lines) |
| `crates/harness-tools/src/tools/apply_patch.rs` | **Done (W2):** empty patch/hunk, malformed header, strip prefix, deletion `/dev/null`, missing patch (~95%) |
| `crates/harness-tools/src/tools/shell.rs` | **Done (W2):** denylist case-insensitive, confirm without run, empty allowlist, def name (~96%) |
| `crates/harness-tools/src/tools/swarm_tool.rs` | **Done (W2):** prompt/count clamps, runner error, def name (100%) |
| `src/swarm.rs` | **Done (W2 residual):** fmt_ts/trunc_chars edges, task_to_json statuses, GC keep-N (~86%) |
| `src/notifications.rs` | **Done (W2 residual):** kind maps + enabled=false / flag no-ops (~76%) |
| `crates/harness-mcp` | **Done (W2 residual):** extract_mcp_text_content edges + allowlist basename exactness |
| `crates/harness-memory` | **Done (W2 residual):** session store CRUD/list/upsert, cosine/search, session short_id |
| `crates/harness-tools/src/tools/git.rs` | **Done climb 2026-08-05** — readonly vs mutating + force-push/missing action/status/stash |
| `crates/harness-tools/src/executor.rs` | **Done climb 2026-08-05** — confirm deny/approve + preview/trust/always_ask pure helpers |
| `src/auth_token.rs` | **Done climb 2026-08-05** — token shape/expiry + `load_or_create_in` / `read_token_file_in` tempdir |
| `src/trust.rs` | **Next (swarm-50)** — path-isolated pure tests (no CWD asserts) |
| `src/projects.rs` | **Next (swarm-50)** — path-isolated pure tests |
| `crates/harness-tools/src/tools/gh.rs` | **Next (swarm-50)** — arg validation + def name |
| `crates/harness-tools/src/tools/search.rs` | **Next (swarm-50)** — validation + def |
| `crates/harness-tools/src/tools/test_runner.rs` | **Next (swarm-50)** — validation edges |
| `crates/harness-tools/src/tools/selfdev.rs` | **Next (swarm-50)** — validation edges |
| `src/agent/*` | Pure message/tool-result + compact/token estimate helpers |
| TUI driver / CLI wiring | Later — near 0%; extract pure helpers before heavy UI tests |

Prefer **unit tests in existing modules** over new integration binaries. No API keys, no live MCP servers, no real desktop notification asserts (disabled-config no-ops only).

## How to remeasure

```bash
# Preferred (matches CI)
cargo install cargo-llvm-cov   # once
cargo llvm-cov --workspace --all-features --summary-only

# Package slice (fast climb check)
cargo llvm-cov -p harness-tools -p harness-mcp -p harness-memory --summary-only

# Alternate (historical COVERAGE.md tool)
cargo tarpaulin --workspace --out Stdout --timeout 300
```

After a fresh run, update **Measured**, **Date**, and tool in `COVERAGE.md`. Keep README badge honest (measured figure or explicit “target 60%”).

## Verification (this wave)

```bash
# ONE filter per cargo test invocation
cargo test --bin harness notifications
cargo test --bin harness swarm
cargo test -p harness-tools
cargo test -p harness-mcp
cargo test -p harness-memory
```

## Notes

- Root package is a **binary**: use `cargo test --bin harness <filter>`, not `--lib`.
- Path has a space (`harness rework`) — quote `cd` / workdir.
- Do not claim the 60% PR gate is green until a real llvm-cov/tarpaulin run supports it.
