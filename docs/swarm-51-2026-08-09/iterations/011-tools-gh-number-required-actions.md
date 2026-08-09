# 011 — tools/gh number-required action validation

**Module:** `crates/harness-tools/src/tools/gh.rs`  
**Slice:** execute arg validation before `run_gh`

## Edges covered
- `pr_diff`, `pr_checks`, `issue_view`, `run_view`, `run_logs` each error when `number` missing
- Error string contains `"number"` (via `require_number`)

## Notes
- Complements existing `pr_view_requires_number` without calling the CLI.
