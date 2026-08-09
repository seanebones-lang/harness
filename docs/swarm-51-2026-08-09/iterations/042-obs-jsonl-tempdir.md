# 042 · obs · JSONL tempfile roundtrip

**Time:** 2026-08-09 swarm-51  

## Work
- Tempdir: write multi-span + multi-file; list newest-first + limit
- Ignore non-`.jsonl`; skip blank/invalid JSONL lines
- Missing id → err; empty dir load_last → `[]`
- Disabled tracer does not require home writes in asserts

## Pitfall
- mtime order: short `thread::sleep` between writes under parallel FS

## Gate
- `cargo test --bin harness observability`
