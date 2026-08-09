# 044 · projects · load_from / save_to

**Time:** 2026-08-09 swarm-51  
**Files:** `src/projects.rs` only

## Work
- `ProjectStore::load_from` / `save_to` path-inject
- `load`/`save` wrap `path()` (home `.harness/projects.json`)
- Missing / invalid / empty JSON → default store
- Nested parent create on `save_to`

## Gate
- tempfile only — no CWD asserts
