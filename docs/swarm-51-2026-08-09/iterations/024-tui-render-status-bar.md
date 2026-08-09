# Iteration 024 · tui · render status_indicators + format_status_bar_line

**Time:** 2026-08-09 swarm-51  
**Root:** `/Users/nexteleven/Desktop/harness rework`  
**Branch:** main · **no commit**

## Work
- Individual flag matrix for `status_indicators` (CU/PLAN/REC/FOCUS/SEARCH/swarm)
- Confirm label ignored unless `plan_mode`; `swarm_active == 0` never badges
- `format_status_bar_line`: pad to width, strict `<` boundary (exact sum → no pad), unicode char-count (not bytes), width 0 still wraps spaces

## Tests (new)
- `status_indicators_individual_flags`
- `format_status_bar_line_padding_and_unicode`
