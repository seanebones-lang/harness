# 013 — tools/search glob_match edges + definition schema

**Module:** `crates/harness-tools/src/tools/search.rs`  
**Slice:** pure glob helper + ToolDefinition schema

## Edges covered
- `*.toml` vs `Cargo.toml.bak` (suffix, not full glob)
- `*.bak` matches multi-dot names; empty name rejects `*.rs`
- Exact-name match case; non-`*.` patterns do not path-glob (`src/*.rs` ≠ `main.rs`)
- Definition `function.name == "search_code"`, required `pattern`, properties present

## Notes
- ToolDefinition name via `function.name` (not top-level `.name`).
