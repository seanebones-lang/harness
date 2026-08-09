# 015 — tools/test_runner schema + extract_number edges

**Module:** `crates/harness-tools/src/tools/test_runner.rs`  
**Slice:** definition properties + parser helper corners

## Edges covered
- `function.name == "test_runner"`; optional `scope` / `timeout_secs`
- No required array (or empty)
- `extract_number`: leading count, bare word, non-numeric token, zero failed, empty line

## Notes
- Pure; no process spawn / cargo test invocation from the tool under test.
