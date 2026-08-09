# 048 · diff_review · hunk pure edges

**Time:** 2026-08-09 swarm-51  

## Work
- multiline swap compute_hunks (+/- ops, headers, Pending)
- stage_write existing file original+hunks
- finalize all-pending → proposed
- empty hunks `apply_accepted_hunks` vacuous all-Reject → original
- glob `.` literal + invalid pattern no panic

## Gate
- extend single existing `mod tests`
