# 016 — tools/test_runner cargo parse without summary + agent FAIL string

**Module:** `crates/harness-tools/src/tools/test_runner.rs`  
**Slice:** `parse_cargo` line-marker path + failure block

## Edges covered
- No `test result:` summary → counts from `... ok` / `... FAILED` lines
- Failure name extracted as `a::three`; default message `"test failed"`
- Single failure + failures section: agent string `[FAIL]` and `0 passed, 1 failed`
- Documents that single-block stdout message flush needs a subsequent header

## Notes
- Pure string fixtures only.
