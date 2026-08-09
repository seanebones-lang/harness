# 040 · obs · path-inject helpers

**Time:** 2026-08-09 swarm-51  
**Root:** `<repo-root>`  
**Branch:** main · **NO COMMIT** (child lane)

## Work
- `default_traces_dir()`, `write_local_trace_in`, `list_traces_in`, `load_last_trace_in`
- `load_trace_file`, `load_trace_by_id` / `_in`; `export_trace` → load helper
- Production wrappers keep `~/.harness/traces`

## Gate
- `cargo test --bin harness observability` (partial; full suite later notes)

## Links
- `src/observability.rs` · [[Swarm-51/Index]]
