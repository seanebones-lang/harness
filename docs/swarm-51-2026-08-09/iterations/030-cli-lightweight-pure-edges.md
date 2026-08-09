# 030 · cli · lightweight pure edges

**Lane:** Swarm-51 residual climb — `src/cli/lightweight.rs` only (not wiring).  
**Date:** 2026-08-09  
**Commit:** none (child does not commit)

## Scope

Expand pure unit tests in existing `mod tests` for MCP path/allowlist helpers.

## Surfaces covered

| Helper | Edges |
|--------|--------|
| `mcp_command_allowed` | defaults npx/node/python3/uvx; path basename; empty allow-all; custom full-path vs basename; case-sensitive; empty/`.exe` reject; single-entry does not widen to defaults |
| `resolve_mcp_config_path_in` | existing explicit wins; missing explicit → discovery; both None; missing + no discovery → None; discovery passthrough even if fake path; explicit wins when both real |
| `load_mcp_servers` | tempfile mcp.json; name sort; default allowlist skips bash; server filter; empty filter miss; empty allowlist allow-all; custom bash-only; missing file error |

## Gate

```bash
cd "<repo-root>"
cargo test --bin harness lightweight
# 6 passed (was 2)
```

## Notes

- No production behavior change.
- `load_mcp_servers` is pure enough with tempfile + `Config::default()` (no network/spawn).
- Skipped live dispatch / MCP spawn / cost DB / swarm registry (side-effectful).
