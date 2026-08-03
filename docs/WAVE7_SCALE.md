# Wave 7 — Scale design notes (max-opt 2026-08-03)

Engineering design for open W7 items. Implementation follows Batch 1 (providers + tools + coverage).

## W7.1 Optional remote/shared swarm registry

**Current:** local SQLite `~/.harness/swarm.db` (`HARNESS_SWARM_DB`).

**Proposal:**
1. Keep SQLite as default; add `[swarm] registry_url` optional HTTP endpoint.
2. Protocol: thin REST compatible with existing task JSON (`task_to_json`):
   - `POST /tasks` spawn
   - `GET /tasks` list
   - `GET /tasks/:id` status/result
   - `POST /tasks/:id/cancel`
   - `POST /gc`
3. Auth: bearer token from env `HARNESS_SWARM_TOKEN`; never commit secrets.
4. Fallback: if `registry_url` set but unreachable → error with local-db tip (no silent split-brain).
5. Ship order: client trait `SwarmRegistry` + `SqliteRegistry` + `HttpRegistry` stub; CLI unchanged.

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

## W7.6 Stable 0.2.0

Gates: Waves 0–2 complete + W1 smoke matrix (W1.4 billing still 📌 may block prebuilts; stable can wait).
