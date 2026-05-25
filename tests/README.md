# Test Suite

All tests in this suite run without API keys. No network calls to LLM providers are made; the tests use mock providers and in-process fakes.

## Running tests

```bash
# Run all workspace tests (recommended)
cargo test --all

# Run only the root integration tests
cargo test

# Run a specific test file
cargo test --test smoke_test
cargo test --test cli_smoke_test
cargo test --test sandbox_tests
cargo test --test error_handling_tests

# Run a single test by name
cargo test --test smoke_test agent_loop_basic

# Run with output from passing tests (useful when debugging)
cargo test --all -- --nocapture

# Run tests in a specific crate
cargo test -p harness-tools
cargo test -p harness-memory
cargo test -p harness-mcp
```

## Coverage

The CI coverage gate requires at least **60% line coverage**. To measure coverage locally:

```bash
# Install the coverage tool (once)
cargo install cargo-llvm-cov

# Generate coverage report
cargo llvm-cov --all

# Generate an HTML report (opens in browser)
cargo llvm-cov --all --html
open target/llvm-cov/html/index.html
```

Coverage is also reported on pull requests via the `.github/workflows/coverage.yml` workflow.

---

## Test files

### `tests/smoke_test.rs`

The primary integration test file. Covers the core functionality of the agent and its subsystems:

- **Agent loop**: drive_agent completes without API keys using a mock provider; tool calls are dispatched and results appended to the session correctly.
- **Memory pipeline**: storing memories, embedding, cosine search returning top-k results, augmenting the system prompt with recalled facts.
- **Session persistence**: saving and reloading sessions from SQLite; finding sessions by ID prefix; listing sessions.
- **Tool execution**: ReadFileTool, WriteFileTool, ListDirTool, PatchFileTool, SearchCodeTool each exercised with temp directories.
- **MCP client**: initialize handshake, tools/list, tools/call round-trip using a fake stdio MCP server.
- **Swarm**: queuing tasks, status transitions (Queued → Running → Done), result retrieval, concurrency limit enforcement.
- **Cost tracking**: recording usage events, daily/monthly budget check, by-model breakdown query.
- **Structured output**: ResponseSchema injected into ChatRequest; mock provider returns JSON matching the schema; output parsed correctly.

### `tests/cli_smoke_test.rs`

Tests the command-line interface and argument parsing:

- **Argument parsing**: all subcommands (`sessions`, `export`, `cost`, `swarm`, `sync`, `models`, `voice`, `serve`, `doctor`, `completions`, `trace`, `pr`) parse without panicking.
- **One-shot mode**: passing a prompt string invokes the agent loop once and exits.
- **`--resume` flag**: session ID prefix lookup wired to the mock session store.
- **`--think N`**: thinking budget flag is forwarded to the ChatRequest.
- **`--plan` flag**: diff-review mode is activated when `--plan` is passed.
- **`harness doctor`**: health-check output includes expected subsystem names.
- **Shell completions**: `harness completions bash/zsh/fish` produces non-empty output.

### `tests/sandbox_tests.rs`

Tests the security and isolation boundaries of tool execution:

- **ShellTool timeout**: a command that sleeps longer than the configured timeout is killed, not hung.
- **ShellTool path constraints**: verifies that the shell tool does not inadvertently escape temp-dir boundaries in basic usage patterns.
- **SpawnAgentTool isolation**: sub-agents spawned via SpawnAgentTool receive only the base tool set (read_file, write_file, list_dir, shell, search_code) and cannot access privileged tools like ComputerUseTool or RebuildSelfTool.
- **ComputerUseTool gating**: ComputerUseTool is not present in the default tool set when `[computer_use] enabled = false` (the default).
- **MCP sampling approval**: a mock MCP server requesting `sampling/createMessage` is blocked until the approval callback returns `true`.
- **Sync file permissions**: `~/.harness/.sync-key` is written with mode 0600 on Unix; the test verifies the mode bits after init.

### `tests/error_handling_tests.rs`

Tests graceful degradation and error propagation:

- **Provider HTTP errors**: a mock provider returning 4xx/5xx responses causes `stream_chat` to return an `Err`, which the agent loop propagates cleanly without panicking.
- **Malformed SSE**: truncated or syntactically invalid SSE frames are skipped with a warning, not a panic.
- **Tool execution errors**: a tool returning `Err` is formatted as a tool-result error message and the agent loop continues to the next turn.
- **Memory embed failure**: when the embedding provider is unavailable, `build_augmented_system` logs a warning and returns the system prompt unmodified rather than aborting the turn.
- **Session load failure**: a corrupt or missing `sessions.db` returns a descriptive error, not an unhandled panic.
- **Budget exceeded**: when `check_budget` returns 100%, the agent emits a `BudgetAlert` notification and continues (the budget is advisory, not a hard kill switch, unless configured otherwise).
- **Swarm task failure**: a sub-agent that panics transitions to `TaskStatus::Failed` with the panic message stored in the result column; the semaphore slot is released.
