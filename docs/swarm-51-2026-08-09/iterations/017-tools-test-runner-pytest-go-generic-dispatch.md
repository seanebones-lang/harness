# 017 — tools/test_runner pytest/go/generic + full Runner dispatch

**Module:** `crates/harness-tools/src/tools/test_runner.rs`  
**Slice:** remaining parsers + `parse_output` match arms

## Edges covered
- Pytest `FAILED` without ` - ` → message `"failed"`
- Pytest garbage + `success=false` → `failed=1` fallback; success keeps zeros
- Go empty output; multi PASS/FAIL name parse
- Generic empty success PASS + “All tests passed”; empty fail → message `"failed"`
- `parse_output` for Npm/Pytest/Go/Cargo
- `Runner` PartialEq equality smoke

## Notes
- Completes pure coverage of parser surface without `detect_test_command` cwd coupling.
