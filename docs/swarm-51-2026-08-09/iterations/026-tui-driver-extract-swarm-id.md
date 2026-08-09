# Iteration 026 · tui · driver extract_swarm_task_id

**Time:** 2026-08-09 swarm-51  
**Root:** `<repo-root>`  
**Branch:** main · **no commit**

## Work
- Extended existing `src/tui/driver.rs` `mod tests`
- Edges: stacked `!*` markers, min len 4 (`sw1` reject / `sw12` accept)
- Leading tab: not in trim set, but `split_whitespace` still yields id
- Leading `-`/`#` glued → reject; case-sensitive `sw`; first token only; whitespace-only / `***`

## Tests (new)
- `extract_swarm_task_id_edge_markers_and_lengths`
