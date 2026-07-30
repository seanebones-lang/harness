# Coverage plan (W2.1)

Short uplift plan for workspace line coverage. **Measured baseline** comes from root [`COVERAGE.md`](../COVERAGE.md) — do not invent higher numbers.

| Field | Value |
|-------|--------|
| **Current measured** | **~23.33%** lines (2407 / 10317; tarpaulin, 2026-05-25) |
| **CI gate (target)** | ≥ 60% lines via `cargo llvm-cov --fail-under-lines 60` — **not met** |
| **Near-term goal** | Workspace **≥ 40%**, then critical crates **≥ 60%** |
| **Stretch** | Meet CI **≥ 60%** workspace |

## Priority order (critical crates first)

1. **`harness-tools`** — tool input validation, denylist/confirm policy, workspace sandbox, swarm/agent enqueue tools, executor gates. High pure-logic density; no network.
2. **Agent / swarm (`src/swarm.rs`, agent loop helpers)** — status labels, GC, task JSON, counts, format helpers; then agent pure branches.
3. **`harness-mcp`** — config parse, command allowlist, text/content helpers, in-process duplex client tests (already strong — keep expanding pure helpers).
4. **Auth / tokens (`src/auth_token.rs`, trust)** — parsing, expiry, allow/deny without live providers.
5. Providers / TUI / CLI — later: harder I/O and UI surface; prefer extracting pure helpers before heavy integration tests.

## Concrete next test modules

| Module | What to cover next |
|--------|--------------------|
| `crates/harness-tools/src/tools/filesystem.rs` | Missing args, line-range clamp, patch uniqueness errors |
| `crates/harness-tools/src/tools/apply_patch.rs` | Parse/apply edge cases (empty hunk, mismatch) |
| `crates/harness-tools/src/tools/git.rs` | Readonly vs mutating action dispatch (args only) |
| `crates/harness-tools/src/executor.rs` | Confirm deny/approve paths with mock tools |
| `src/swarm.rs` | Keep GC keep-N + concurrent cancel edges green |
| `src/agent.rs` | Pure message/tool-result formatting helpers |
| `src/auth_token.rs` | Token shape / expiry helpers |
| `crates/harness-mcp/src/client.rs` | More `extract_mcp_text_content` / sampling approval unit paths |
| `crates/harness-memory` | Session store CRUD on temp DB (no network) |

Prefer **unit tests in existing modules** over new integration binaries. No API keys, no live MCP servers, no real desktop notification asserts (disabled-config no-ops only).

## How to remeasure

```bash
# Preferred (matches CI)
cargo install cargo-llvm-cov   # once
cargo llvm-cov --workspace --all-features --summary-only

# Alternate (historical COVERAGE.md tool)
cargo tarpaulin --workspace --out Stdout --timeout 300
```

After a fresh run, update **Measured**, **Date**, and tool in `COVERAGE.md`. Keep README badge honest (measured figure or explicit “target 60%”).

## Verification (this wave)

```bash
cargo test --bin harness notifications swarm
cargo test -p harness-tools
cargo test -p harness-mcp
```

## Notes

- Root package is a **binary**: use `cargo test --bin harness <filter>`, not `--lib`.
- Path has a space (`harness rework`) — quote `cd` / workdir.
- Do not claim the 60% PR gate is green until a real llvm-cov/tarpaulin run supports it.
