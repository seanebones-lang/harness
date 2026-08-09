# 047 · diff_review · patch_file + commit

**Time:** 2026-08-09 swarm-51  

## Work
- `file_diff_from_tool("patch_file")` happy / no-match / missing fields / missing file
- `StagingBuffer::commit`: file_decision accept/reject; hunk accept write; all-reject keep original
- New-file hunk header contains `new file`

## Gate
- tempfile only
