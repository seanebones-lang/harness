# Iteration 028 · tui · driver fork_session_at

**Time:** 2026-08-09 swarm-51  
**Root:** `/Users/nexteleven/Desktop/harness rework`  
**Branch:** main · **no commit**

## Work
- Beyond available turns → full transcript copy; new id; name `… (fork@N)`
- Stops when `user_count >= turn_n` (includes messages up to that user; tool between users kept when n covers later user only after earlier prefix)
- `turn_n=0`: after first message `user_count >= 0` → single leading msg
- Leading system before first user; empty session model copy; named fork@0

## Tests (new)
- `fork_session_at_beyond_available_and_preserves_prefix`
- `fork_session_at_turn_zero_and_leading_non_user`
- `fork_session_at_empty_and_model_copy`
