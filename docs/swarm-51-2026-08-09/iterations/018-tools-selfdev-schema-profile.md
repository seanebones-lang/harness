# 018 — tools/selfdev definition schema + profile/src_dir constructors

**Module:** `crates/harness-tools/src/tools/selfdev.rs`  
**Slice:** pure constructors + ToolDefinition shapes

## Edges covered
- Rebuild schema exposes `check_only` boolean property + description
- Reload schema: `type=object`, empty `properties`
- `with_profile("dev")` and empty-string profile override
- Rebuild vs Reload independent `src_dir` values

## Notes
- No cargo build / exec; no network.
