# Iteration 021 · tui · render compute_chat_items

**Time:** 2026-08-09 swarm-51  
**Root:** `<repo-root>`  
**Branch:** main · **no commit**

## Work
- Edge coverage for `compute_chat_items` / `compute_chat_items_from`
- Empty content messages (`max(1)` row), multiline + empty event roles
- Busy-only streaming row, busy+stream non-double-count, blank lines in stream buffer
- Direct `compute_chat_items_from` busy-without-stream matrix

## Tests (new)
- `compute_chat_items_empty_content_and_multiline_event`
- `compute_chat_items_streaming_empty_with_busy_and_both`
- `compute_chat_items_from_busy_without_stream`
