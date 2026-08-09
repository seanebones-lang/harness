# 012 — tools/gh whitespace-only message/title rejects

**Module:** `crates/harness-tools/src/tools/gh.rs`  
**Slice:** trim-empty guards on `pr_comment` / `pr_create`

## Edges covered
- `pr_comment` with whitespace-only `message` → error contains `"message"`
- `pr_comment` with whitespace-only `body` alias → same error path
- `pr_create` with whitespace-only `title` → error contains `"title"`

## Notes
- Pure validation; never reaches `run_gh`.
