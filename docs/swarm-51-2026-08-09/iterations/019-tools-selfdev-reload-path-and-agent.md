# 019 — tools/selfdev reload path segments + agent pure edges

**Modules:**
- `crates/harness-tools/src/tools/selfdev.rs`
- `crates/harness-tools/src/tools/agent.rs`

## selfdev edges
- Missing binary error includes `target` / `selfdev` / `harness` path segments
- Creating empty `target/selfdev/` dir still errors (`Binary not found`)
- Extra JSON keys ignored on execute

## agent edges
- `function.name == "spawn_agent"`; required `task`; properties for task/context
- Missing task / non-string task → `"missing task"` (runner not invoked)
- Absent or empty `context` → prompt is task only (no “Additional context”)
- Non-empty context appends `\n\nAdditional context:\n…`
- Runner `anyhow::bail!` propagates

## Verify
- `cargo test -p harness-tools` → **179 passed** (was **148**; **+31**)
- DO NOT COMMIT (swarm child lane)
