# 50-iteration plan (micro slices)

Each iteration = one focused ship unit: tests, honesty docs, or hygiene. Non-overlapping paths within a wave.

## Wave A — Hygiene + truth (1–6)
1. Delete junk `src/swarm 2.rs`, `CLAUDE 2.md` if safe
2. CTO exec findings: Gemini/Bedrock closed; coverage 46.52%
3. Vault Index → main + tip + Swarm-50 link
4. HQ Projects/Harness last session + swarm link
5. Activity Log prepend start
6. Baseline PROGRESS.md + test count snapshot

## Wave B — Pure unit climb src (7–18)
7. `trust.rs` path-isolated tests
8. `projects.rs` path-isolated tests
9. `rate_limit.rs` edge cases
10. `highlight.rs` parse_blocks edges
11. `cost.rs` pure format/estimate
12. `diff_review.rs` more hunk edges
13. `bridges.rs` more pure helpers
14. `bench.rs` pack load edges
15. `auth_token.rs` residual edges
16. `observability.rs` list/load pure
17. `tui/theme.rs` pure style helpers
18. `tui/slash.rs` more detect_* cases

## Wave C — harness-tools (19–30)
19. `gh.rs` arg validation + def name
20. `search.rs` validation + def
21. `selfdev.rs` validation
22. `test_runner.rs` validation
23. `computer.rs` extra pure branches
24. `database.rs` readonly SQL edges
25. `notebook.rs` structure edges
26. `docker.rs` validate_docker edges
27. `policy.rs` new action classifications
28. `executor.rs` residual pure
29. `workspace_root.rs` edges
30. `confirm.rs` edges

## Wave D — agent/server/swarm residual (31–38)
31. agent compact/token estimate edges
32. agent naming pure
33. agent system load_project_instructions fixture
34. server auth extract_bearer edges
35. server project_ops pure helpers
36. swarm_registry LocalSqlite residual
37. swarm fmt/gc residual
38. notifications residual no-op

## Wave E — Docs honesty + smoke (39–46)
39. README coverage badge align
40. COVERAGE_PLAN next modules refresh
41. RELEASE_STATUS swarm-50 row
42. TEAM_UPDATE_2026-08-05 stub
43. SHORTCUTS / COOKBOOK spot-check vs `./target/debug/harness`
44. Offline smoke_rel01 re-run
45. Clippy -D warnings bin
46. cargo test -p harness-tools full

## Wave F — Integrate + remeasure (47–50)
47. Full `cargo test --bin harness` gate
48. llvm-cov summary remeasure → COVERAGE.md
49. Commit + push main (if green)
50. Close notes + Obsidian wikilinks + HQ log

## Acceptance
- 50 iteration notes written
- No secrets
- Gates green or honest blockers logged
- Obsidian linked
