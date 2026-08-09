# 045 · projects · store edge tests

**Time:** 2026-08-09 swarm-51  

## Work
- Infer name from path basename
- Update preserves `default_branch` when None + no git
- Remove by path; missing target None
- `list_sorted` empty; entry serde; `path()` suffix
- `detect_git_remote` / `detect_default_branch` None on plain tempdir
- canonicalize existing tempdir absolute

## Gate
- `cargo test --bin harness projects` (also matches bridges github_projects*)
