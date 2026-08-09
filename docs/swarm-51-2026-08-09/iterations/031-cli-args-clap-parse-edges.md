# 031 · cli · args clap pure parse edges

**Lane:** Swarm-51 residual climb — `src/cli/args.rs` pure clap surfaces only (not wiring).  
**Date:** 2026-08-09  
**Commit:** none (child does not commit)

## Scope

New `#[cfg(test)] mod tests` using `Cli::try_parse_from` — no runtime, no filesystem side effects.

## Tests added (7)

| Test | Covers |
|------|--------|
| `top_level_defaults_and_global_flags` | bare `harness`; `--no-memory/--browser/--plan/-v/--model/--think/--browser-url/--resume` + positional prompt |
| `parse_run_serve_export_delete_and_init` | Run; Serve default+override addr; Export `-o`; Delete; Init `--project --force` |
| `parse_swarm_run_aliases_status_json_cancel_all_gc` | `-n` + `--agents` alias; model; status/result `--json`; cancel id/`--all`; wait default 300; gc stale/keep/older/dry-run; list |
| `parse_mcp_bridge_cost_project_aliases_and_bench` | mcp resources/read/roots; bridge obsidian; cost by-model; `proj ls` + `project new`; bench `--json --pack`; voice `-d` + default 5; pr `--comment` |
| `parse_rejects_unknown_subcommand_and_missing_required` | bare unknown → prompt (not error); bad swarm action; missing required; project sync target+`--all` conflict |
| `parse_checkpoint_sync_trust_trace_and_completions_shell` | checkpoint list; sync init; trust; trace id/None; completions bash/zsh/fish |
| `parse_project_exec_trailing_and_publish_flags` | exec trailing after `--`; publish `--public --repo` |

## Gate

```bash
cd "/Users/nexteleven/Desktop/harness rework"
cargo test --bin harness args
# 7 passed
```

## Pitfalls discovered

1. **`Cli` has no `Debug`** — cannot use `Result::expect_err`; use `match try_parse_from`.
2. **`Commands::Connect` clap debug-assert** — defaulted positional `url` before required `prompt` panics in debug builds when that subcommand is constructed. Live parse skipped; production shape left unchanged (out of lane).
3. **Bare unknown token is a prompt**, not a parse error — asserted intentionally.
4. Parallel agent had briefly broken `setup.rs` tests with `ProviderConfig` vs `ProviderEntry` (sibling fixed to `or_default()`).

## Out of scope

- `wiring.rs`
- Production clap shape fix for `Connect`
- Commit
