# 014 — tools/search arg types, max_results=0, sandbox escape

**Module:** `crates/harness-tools/src/tools/search.rs`  
**Slice:** execute validation + strict workspace path

## Edges covered
- Non-string `pattern` (number) → `"missing pattern"`
- `max_results: 0` with hits on disk → `"No matches"` (zero budget)
- Absolute path outside TempDir workspace → resolve error (strict sandbox)
- Invalid UTF-8 file skipped; UTF-8 neighbor still matched

## Notes
- TempDir + `SandboxMode::Strict` only; no network.
