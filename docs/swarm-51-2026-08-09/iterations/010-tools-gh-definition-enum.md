# 010 — tools/gh definition enum + require_number edges

**Module:** `crates/harness-tools/src/tools/gh.rs`  
**Slice:** pure definition/schema + `require_number` formatting

## Edges covered
- `definition().function.name` already covered; new: `required` includes `action`
- Action `enum` lists all 10 supported actions (`pr_list` … `run_logs`)
- Description mentions GitHub CLI
- `require_number(Some(0))` → `"0"`; `Some(u64::MAX)` stringifies fully

## Notes
- No live `gh` / network. Single `mod tests` retained.
- Baseline package: 148 → intermediate climb toward 179.
