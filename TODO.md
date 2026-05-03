# Harness — Remaining Work

Canonical **user** docs: `README.md`, `SEAN START HERE/USER MANUAL.md`; **developer**: `CLAUDE.md`, `config/default.toml`.

Older **Critical / Important** items (xAI `stream_options` + usage, multi-tool-call
streaming tests, embedding retries, TUI `--resume`, ambient `ctrl_c` shutdown, web UI
session persistence, session delete CLI, clippy cleanliness) are **implemented** in tree.

This file tracks follow-ups that are still worth doing.

Release readiness: **[`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md)** · latest verdict: **[`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md)**

---
## Polish

### Documentation (README / USER MANUAL / `CLAUDE.md`)

Screenshots or deeper CDP troubleshooting are optional polish.

### Architecture: generic `ambient` provider

**File:** `src/ambient.rs`

`spawn()` and `consolidate()` take `XaiProvider` directly. Prefer
`P: Provider + Clone + 'static` so non-xAI backends can reuse ambient consolidation.

### Testing gaps

**`harness-browser`** (`crates/harness-browser/`): unit tests for no-Chrome error path,
unknown `BrowserTool::execute` action, CDP JSON round-trip.

**`ambient.rs`** (`src/ambient.rs` or `tests/smoke_test.rs`): consolidation with a mock
`MemoryStore` (e.g. ≥ 5 entries) and checks on merged / `__consolidated__` entries.

### `harness sessions` vs async auto-naming

**File:** `src/main.rs` (`list_sessions()`)

Titles may lag the first list after save because naming is async; optional re-query after
rename or note the limitation in UX copy.

## Testing checklist before release

Maintainers: use **[`docs/PUBLIC_RELEASE.md`](../docs/PUBLIC_RELEASE.md)** for the full public checklist (legal, gates, manual smokes, quickstart rehearsal, go/no-go).

Automated gates (verified in dev; **CI** runs the same on **Ubuntu, macOS, Windows**):

- [x] `cargo test --all` — workspace integration + doctests (no API keys)
- [x] `cargo clippy --all-targets --all-features -- -D warnings`
- [x] `cargo fmt --all -- --check`
- [x] `cargo build --profile release-lto`

Manual (needs API keys / local GUI):

- [ ] `XAI_API_KEY=... harness "list files in ."` — one-shot works
- [ ] `XAI_API_KEY=... harness` — TUI, token counts in status bar
- [ ] `XAI_API_KEY=... harness serve` + `http://127.0.0.1:8787` — web UI chat
- [ ] `harness export <id>` — Markdown OK in a viewer
- [ ] `harness sessions` — lists sessions (including auto-named when ready)
