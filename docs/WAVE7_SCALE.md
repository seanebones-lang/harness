# Wave 7 — Scale design notes (max-opt 2026-08-03)

Engineering design for open W7 items. Implementation follows Batch 1 (providers + tools + coverage).

## W7.1 Optional remote/shared swarm registry

**Current:** local SQLite `~/.harness/swarm.db` (`HARNESS_SWARM_DB`).

**Shipped (2026-08-04):**
1. SQLite default; `[swarm] registry_url` selects `HttpRegistry`.
2. REST client (sync blocking reqwest):
   - `POST /tasks` — `{prompt, model?}` → task JSON (`task_to_json` shape)
   - `GET /tasks?limit=N` — `{tasks:[…]}` or bare array
   - `GET /tasks/:id` — task or 404
   - `PUT /tasks/:id` — `{status, result?, error?}`
3. Auth: `Authorization: Bearer $HARNESS_SWARM_TOKEN`
4. Unreachable → hard error + tip to unset `registry_url` (no split-brain)
5. Tests: pure JSON parse + in-process axum mock server roundtrip + unreachable

**Cutover (2026-08-05):** public `register_task*` / `list_tasks` / `get_task` / `update_status`
route through HTTP when `registry_url` is set (or `HARNESS_SWARM_REGISTRY_URL` env).
Local SQLite helpers stay under `*_local` for `LocalSqliteRegistry` + GC spawn paths.
`swarm list` prints `registry: sqlite-local|http-remote`.

**Server side** of the registry is still external; this is the client.

## W7.2 Per-worker tool allowlist + quota

**Proposal:**
- `[swarm] worker_tool_allowlist = ["read_file","search_code",…]`
- `[swarm] worker_max_tool_calls = 50`
- `[swarm] worker_max_wall_secs = 600`
- Enforce in swarm spawn path before tool executor build (mirror subagent readonly registry in `wiring.rs`).
- Default allowlist = read-only tools (no shell/write/git push).

## W7.3 Benchmark harness

**Existing:** `BENCHMARKS.md`, `benches/`.

**Proposal:**
- Fixed task pack under `demo/bench_tasks/` (no API keys required for tool-path benches).
- Optional live provider bench gated `HARNESS_BENCH_LIVE=1`.
- Metrics JSON: latency p50/p95, tool calls, tokens if present, cost from cost_db.
- CLI: `harness bench run --pack demo/bench_tasks` (future; until then scripts).

## W7.4 God-file split

Priority extract (stable APIs, no behavior change):
1. `src/agent.rs` → `agent/{loop,memory_inject,tool_dispatch}.rs`
2. `src/server.rs` → `server/{routes,sse,state}.rs`
3. Heavy TUI → already modular under `src/tui/`; finish leftover god modules only if still oversized

## W7.5 Threat model v2

After feature freeze of W5 tools + providers; update `SECURITY.md` checklist.

## W7.6 Supported stable cut (historical working name: 0.2.0)

Gates: Waves 0–2 complete + W1 smoke matrix (W1.4 billing still 📌 may block prebuilts; stable can wait). The repository later moved to version 1.3.0 as a public POC, so do not use the old 0.2.0 working name for the next release.
