# Iteration 025 · tui · render prefix_line

**Time:** 2026-08-09 swarm-51  
**Root:** `<repo-root>`  
**Branch:** main · **no commit**

## Work
- `prefix_line_empty_line_still_gets_prefix`
- Documented real ratatui behavior: `Line::from("")` has **zero** spans → result is sole prefix span
- Empty `Span::raw("")` is preserved after prefix (2 spans)

## Tests (new)
- `prefix_line_empty_line_still_gets_prefix`
