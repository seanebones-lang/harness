# Iteration 023 · tui · render input_bar_title

**Time:** 2026-08-09 swarm-51  
**Root:** `/Users/nexteleven/Desktop/harness rework`  
**Branch:** main · **no commit**

## Work
- Precedence: tab-complete > search > history > default help string
- History display is 1-based index of 0-based `history_idx`
- Empty tab token / empty search query still enter those modes

## Tests (new)
- `input_bar_title_precedence_and_history_index`
