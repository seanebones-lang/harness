# 046 · diff_review · AutoTrust path-inject

**Time:** 2026-08-09 swarm-51  
**Files:** `src/diff_review.rs` only

## Work
- `AutoTrustPatterns::load_from(path)` + pure `from_toml_str`
- `load()` → home `diff-trust.toml` via `load_from`
- Missing/invalid toml → default; non-string array elems skipped

## Gate
- tempfile fixtures; no HOME mutation
