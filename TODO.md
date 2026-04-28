# Harness — Remaining Work

Items are grouped by severity. Fix **Critical** issues before any production use;
**Important** issues affect feature correctness; **Polish** items are quality-of-life.

---

## Critical

### 1. `stream_options: include_usage` not sent to xAI API
**File:** `crates/harness-provider-xai/src/types.rs`, `client.rs`

The `usage` field in SSE chunks is only returned by xAI when the request includes:
```json
"stream_options": { "include_usage": true }
```
Without it, `Delta::Usage` is never emitted and the token tracking feature is a
silent no-op. Add `stream_options` to `ApiRequest` and set it to `true` whenever
`stream: true`.

---

### 2. Only the first tool call per turn is emitted
**File:** `crates/harness-provider-xai/src/stream.rs` — `flush_tool_calls`

`flush_tool_calls` returns all assembled calls, but the caller in `parse_event`
previously only took `calls.into_iter().next()`. The queue-based rewrite pushes
all calls before `Done`, but this path needs an explicit unit test to guard against
regression. Add a smoke test that sends a response with two simultaneous tool calls
and asserts both `Delta::ToolCall` values are received.

---

## Important

### 3. Clippy warnings (8 warnings across 4 files)
Run `cargo clippy --all-targets -- -D warnings` and fix all warnings before merging
to main. Current violations:

| File | Warning |
|---|---|
| `crates/harness-tools/src/tools/filesystem.rs:214` | loop var only used to index |
| `crates/harness-browser/src/session.rs:89` | useless `String` conversion |
| `src/agent.rs:231` | too many arguments (9 > 7) |
| `src/agent.rs:256` | explicit closure for clone |
| `src/tui.rs:160` | too many arguments (10 > 7) |
| `src/tui.rs:206` | explicit closure for clone |
| `src/tui.rs:271` | explicit closure for clone |
| `src/main.rs:535` | literal with empty format string |

### 4. Embed calls have no retry logic
**File:** `crates/harness-provider-xai/src/embed.rs`

`embed_text` makes a plain `reqwest` call with no retry. If xAI rate-limits an
embedding request (e.g. during memory recall or consolidation) the entire agent
turn fails. Apply the same exponential-backoff loop from `stream_chat`.

### 5. TUI cannot resume an existing session
**File:** `src/tui.rs` — `run()` and `event_loop()`

The `--resume` flag is parsed in `Cli` but only wired into `run_once`. The TUI
always creates a `Session::new(model)`. Add a `resume: Option<String>` parameter
to `tui::run()`, resolve it via `session_store.find()` at startup, and pre-populate
the chat panel with the session's existing messages.

### 6. Ambient task not cancelled on clean shutdown
**File:** `src/main.rs`

`_ambient_shutdown` (a `watch::Sender<()>`) is held in a local that is dropped at
the end of `main` — but by then `tokio::main` is already shutting down the runtime
anyway, so the task never receives the cancel signal gracefully. Wire it into a
`tokio::signal::ctrl_c()` handler so the background task can finish its current
consolidation cycle before exit.

### 7. Web UI loses session ID on page refresh
**File:** `static/index.html`

`sessionId` is a plain JS variable; it is lost when the page reloads. Persist it
in `localStorage` (key: `harness_session_id`) and restore it on load. Add a
"New session" button to clear it intentionally.

### 8. No way to delete a session from the CLI
**File:** `src/main.rs`, `crates/harness-memory/src/store.rs`

Add `harness sessions delete <id>` (or `harness delete <id>`) subcommand.
Requires adding `SessionStore::delete(id: &str) -> Result<()>` — a single
`DELETE FROM sessions WHERE id = ?1` query.

---

## Polish

### 9. `harness-browser` not documented in CLAUDE.md
**File:** `CLAUDE.md`

The crate layout table and key-types section do not mention `harness-browser`,
`BrowserSession`, `BrowserTool`, or the `--browser` / `[browser]` config.
Add a row to the workspace layout tree and a short section under Key types.

### 10. `harness export` and `harness-browser` missing from CLAUDE.md
**File:** `CLAUDE.md`

The Running section only shows the original subcommands. Add examples for:
```bash
harness export abc12345
harness export abc12345 --output session.md
harness --browser "screenshot the homepage of example.com"
```

### 11. `ambient.rs` uses concrete `XaiProvider` instead of `Provider` trait
**File:** `src/ambient.rs`

`spawn()` and `consolidate()` take `XaiProvider` directly. Switching to
`P: Provider + Clone + 'static` would allow other providers in future and is
consistent with how `agent.rs` uses `&dyn Provider` via `XaiProvider` today.

### 12. No smoke tests for `harness-browser` crate
**File:** `crates/harness-browser/`

The crate has zero tests. At minimum add unit tests for:
- `BrowserSession::find_or_open_target` error path (no Chrome running)
- `BrowserTool::execute` with unknown `action` value
- The CDP JSON serialisation round-trip

### 13. No smoke tests for `ambient.rs`
**File:** `src/ambient.rs` (or `tests/smoke_test.rs`)

The consolidation logic (`consolidate()`) is untested. Add a test using a
mock `MemoryStore` with ≥ 5 entries and verify that after a consolidation pass
the entry count decreases and a `__consolidated__` entry exists.

### 14. `harness sessions` output doesn't show session name for auto-named sessions
**File:** `src/main.rs` — `list_sessions()`

`list_sessions` prints the name column but auto-naming runs asynchronously after
the immediate `session_store.save()`. The first listing after a session may show
a blank name even though a name was eventually set. This is a race; acceptable for
now but worth a note or a re-query on the final save path.

### 15. `config/default.toml` example not updated for new tools in agent prompt
**File:** `src/main.rs` — `DEFAULT_SYSTEM`, `config/default.toml`

`DEFAULT_SYSTEM` and the annotated config prompt don't mention `patch_file`,
`browser`, or `spawn_agent`. Update both so the agent (and users configuring a
custom prompt) know all available tools.

---

## Testing checklist before release

- [ ] `cargo test` — all 13 tests pass
- [ ] `cargo clippy --all-targets -- -D warnings` — zero warnings
- [ ] `cargo fmt --all -- --check` — no formatting drift
- [ ] `cargo build --profile release-lto` — release binary builds
- [ ] Manual: `XAI_API_KEY=... harness "list files in ."` — one-shot works
- [ ] Manual: `XAI_API_KEY=... harness` — TUI launches, sends a message, token counts appear in status bar
- [ ] Manual: `XAI_API_KEY=... harness serve` + open browser at `http://127.0.0.1:8787` — web UI loads and chat works
- [ ] Manual: `harness export <id>` — Markdown renders correctly in a viewer
- [ ] Manual: `harness sessions` — lists sessions with auto-generated names
