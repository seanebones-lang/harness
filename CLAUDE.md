# harness — Codebase Guide (April 2026)

Rust coding agent. Multi-provider (Anthropic Claude 4.x, xAI Grok 4.x, OpenAI GPT-5.x, Ollama Qwen3-Coder). Fast, low-memory, multi-agent.

## Build & Test

```bash
cargo build                        # dev build
cargo build --profile selfdev      # fast self-modification build
cargo build --profile release-lto  # distribution build (thin LTO, stripped)
cargo test                         # runs tests/smoke_test.rs (no API key needed)
cargo clippy --all-targets -- -D warnings
cargo fmt --all
```

## Running

```bash
# Interactive TUI (claude-sonnet-4-6 is the default)
ANTHROPIC_API_KEY=sk-ant-... harness

# With extended thinking
ANTHROPIC_API_KEY=sk-ant-... harness --think 10000

# xAI Grok 4.20
XAI_API_KEY=xai-... harness --model grok-4.20-0309-reasoning

# One-shot
ANTHROPIC_API_KEY=sk-ant-... harness "refactor src/agent.rs to use a state machine"

# Resume a session
harness --resume abc12345 "continue where we left off"

# HTTP server
harness serve --addr 127.0.0.1:8787

# Cost dashboard
harness cost today
harness cost by-model

# Cross-machine sync
harness sync init git@github.com:user/harness-state.git
harness sync push

# Models picker
harness models
harness models --set anthropic:claude-opus-4-7

# GitHub PR review
harness pr 123

# Project memory
harness memorize architecture "monorepo: crates/ + src/"
harness memories

# Voice input (one-shot)
harness voice

# Self-development mode (agent edits itself)
harness self-dev --src . --model claude-sonnet-4-6
```

## Workspace layout

```
harness/
├── src/                            root binary
│   ├── main.rs                     CLI (clap), tool wiring, self-dev entry
│   ├── agent.rs                    core agentic loop + memory injection
│   ├── tui.rs                      two-panel ratatui TUI
│   ├── highlight.rs                syntect → ratatui syntax highlighting
│   ├── server.rs                   axum HTTP/SSE server (harness serve)
│   ├── events.rs                   AgentEvent enum + channel helpers
│   ├── config.rs                   TOML config structs
│   ├── cost_db.rs                  SQLite cost tracking (~/.harness/cost.db)
│   ├── memory_project.rs           .harness/memory/ project facts
│   ├── sync.rs                     age-encrypted cross-machine git sync
│   └── notifications.rs            desktop notification helpers (notify-rust)
├── crates/
│   ├── harness-provider-core/      Provider trait, Message/Delta/Tool types
│   ├── harness-provider-anthropic/ Claude Sonnet/Opus/Haiku + prompt caching + thinking
│   ├── harness-provider-openai/    GPT-5.x streaming SSE client
│   ├── harness-provider-xai/       Grok 4.x streaming + native tools
│   ├── harness-provider-ollama/    Local Ollama (Qwen3-Coder 30B default)
│   ├── harness-provider-router/    Smart multi-provider router (env-key detection)
│   ├── harness-tools/              Tool trait + built-ins (shell/gh/computer/file/search)
│   ├── harness-memory/             SQLite session store + vector memory store
│   ├── harness-mcp/                MCP stdio protocol client + tool adapter
│   ├── harness-browser/            Chrome CDP browser tool
│   └── harness-voice/              Whisper transcription (OpenAI API + local whisper.cpp)
├── config/default.toml             Annotated default configuration
├── tests/smoke_test.rs             Integration tests (no API key required)
└── scripts/install.sh              Install from source to ~/.local/bin
```

## April 2026 Model Defaults

| Provider  | Default model                    | Notes                         |
|-----------|----------------------------------|-------------------------------|
| Anthropic | `claude-sonnet-4-6`              | Prompt caching on by default  |
| OpenAI    | `gpt-5.5`                        | 1M context, Dec 2025 cutoff   |
| xAI       | `grok-4.20-0309-reasoning`       | 2M context, native tools      |
| Ollama    | `qwen3-coder:30b`                | Local, 256K ctx, SWE-bench RL |
| Embed     | `nomic-embed-text` (Ollama)      | RAG default                   |

Smart router picks the best available provider based on which `*_API_KEY` env vars are set:
`ANTHROPIC_API_KEY` → `XAI_API_KEY` → `OPENAI_API_KEY` → `ollama` (local).

## Key types

### `harness-provider-core`

| Type | Description |
|------|-------------|
| `Provider` trait | `async fn stream_chat(req: ChatRequest) -> DeltaStream` |
| `Message` | `{role: Role, content: MessageContent, tool_call_id?}` |
| `Delta` | `Text(String)` \| `ToolCall(ToolCall)` \| `Done{stop_reason}` \| `CacheUsage{creation, read}` \| `Usage{input, output}` |
| `ToolDefinition` | OpenAI-format function schema |
| `ChatRequest` | Builder: `.with_messages()`, `.with_tools()`, `.with_system()`, `.thinking_budget`, `.native_web_search` |

`ChatRequest` new fields (April 2026):
- `thinking_budget: Option<u32>` — adaptive thinking budget (Anthropic)
- `native_web_search: bool` — enable provider-native web search
- `native_code_execution: bool` — enable sandboxed code execution
- `native_x_search: bool` — enable xAI X (Twitter) search

### `harness-provider-anthropic`

Prompt caching: attaches `cache_control: {type: "ephemeral"}` to system, tools, pinned `@file` messages, and the second-to-last user message. Parses `cache_creation_input_tokens` / `cache_read_input_tokens` from SSE and emits `Delta::CacheUsage`.

Extended thinking: if `thinking_budget` > 0 on the request, sends `thinking: {type: "enabled", budget_tokens: N}` in the body + `anthropic-beta: interleaved-thinking-2025-05-14` header.

### `harness-provider-router`

Auto-builds providers from env keys when no `[providers]` block is configured. Smart routing:
- `default` → anthropic > xai > openai > ollama
- `fast` → same priority, uses fast/cheap model
- `heavy` → same priority, uses opus/grok-reasoning
- `embed` → ollama:nomic-embed-text if available

### `harness-tools`

`Tool` trait: `fn definition() -> ToolDefinition` + `async fn execute(args: Value) -> Result<String>`.

Built-in tools:
- `ReadFileTool`, `WriteFileTool`, `ListDirTool`
- `PatchFileTool` — surgical old→new text replacement with diff output
- `ShellTool` — runs `sh -c <command>`, configurable timeout
- `SearchCodeTool` — regex over gitignore-aware file walk
- `SpawnAgentTool` — runs a sub-agent with base tools only
- `RebuildSelfTool`, `ReloadSelfTool` — self-modification
- `GhTool` — `gh` CLI wrapper (pr_list, pr_view, pr_diff, pr_checks, pr_comment, issue_list, run_view, run_logs)
- `ComputerUseTool` — Anthropic computer-use-2025-01-24 spec (screenshot, mouse, keyboard) — only registered when `[computer_use] enabled = true`

### `harness-voice`

`record_and_transcribe()` — captures audio via `sox rec` / `afrecord`, transcribes via OpenAI Whisper API or local `whisper-cli`. `WhisperBackend::detect()` picks the best available backend.

### `harness-memory`

`SessionStore` — SQLite WAL, sessions table. `save/load/find(prefix-or-name)/list`.
`MemoryStore` — memories table with JSON float embeddings. `insert/search(cosine-similarity, top-k)`.

`memory_project` module — `.harness/memory/<topic>.md` project facts. `remember/forget/list_topics/augment_system`.

### `cost_db`

`CostDb` — SQLite at `~/.harness/cost.db`. Schema: `(session_id, project, provider, model, ts, in_tok, cached_in, out_tok, native_calls, usd)`. `check_budget(db, daily_usd, monthly_usd)` returns `(Option<pct>, Option<pct>)`.

### `sync`

`init(git_url)` / `push()` / `pull()` / `status()` — age-encrypted sync of `~/.harness/{sessions.db, memory.db, trust.json, cost.db, memory/}` to a private git repo. Passphrase stored in macOS Keychain via `security` CLI, falls back to `~/.harness/.sync-key` (mode 0600).

### `notifications`

`notify(cfg, summary, body)` — thin wrapper over `notify-rust`. Three semantic hooks:
`background_done`, `autotest_failed`, `budget_alert`. Test via `/notify test` in TUI or any of the hook functions.

### `harness-mcp`

`McpClient::spawn(name, config)` — forks process, runs initialize handshake, exposes `list_tools()` and `call_tool()`.
`load_mcp_tools(path, registry)` — reads `mcp.json`, registers all server tools automatically.

## Agent loop (`src/agent.rs`)

```
drive_agent(provider, tools, memory?, embed_model?, session, system_prompt, events?) -> Result<()>
  │
  ├─ build_augmented_system()   embed last user msg → cosine search → inject top-3 memories
  │                             + augment with .harness/memory/ project facts
  │
  └─ loop:
       stream_chat(req) → Delta stream
         TextChunk    → emit event, buffer text
         ToolCall     → collect into pending list
         Done         → break or set stop_reason
         CacheUsage   → emit AgentEvent::CacheUsage
         Usage        → emit AgentEvent::TokenUsage → record to cost.db
       push assistant message to session
       for each tool call:
         emit ToolStart
         executor.execute(call)
         emit ToolResult
         push tool_result message to session
       if no tool calls → break
```

## TUI layout (`src/tui.rs`)

```
┌──────────────── Chat (62%) ──────────────────┬─── Tools & Events (38%) ──┐
│ ┌ [you]                                       │  → read_file              │
│ │ refactor the agent loop                     │  ← read_file: pub async f…│
│                                               │  → shell                  │
│ ┌ [claude] ●  (streaming, yellow)             │  ← shell: Build succeeded │
│ │ Here's the refactored version…              │  memory: recalled 2 entries│
├─────────────────────────────────────────────┴───────────────────────────┤
│  Message: _                                                               │
├───────────────────────────────────────────────────────────────────────────┤
│  Session abc · claude-sonnet-4-6 · 4 turns · $0.003 · cache:73%↑        │
└───────────────────────────────────────────────────────────────────────────┘
```

Status bar shows: session ID · model · turns · cost · cache hit rate · `[COMPUTER USE LIVE]` / `[🎙 REC]` when active.

Keys: `Enter` send · `↑↓` scroll chat · `PgUp/PgDn` scroll event log · `Ctrl+V` voice · `Ctrl+C` quit.

## HTTP API (`src/server.rs`)

```
POST /api/chat          body: {prompt, session_id?}   → SSE AgentEvent stream
GET  /api/sessions      → [{id, name, updated_at}]
GET  /api/sessions/:id  → full Session JSON
GET  /api/health        → {status, model}
```

SSE event types: `text_chunk`, `tool_start`, `tool_result`, `memory_recall`,
`cache_usage`, `token_usage`, `sub_agent_spawned`, `sub_agent_done`, `done`, `error`.

## Configuration (`~/.harness/config.toml`)

```toml
[provider]
api_key = "sk-ant-..."             # or ANTHROPIC_API_KEY env var
model = "claude-sonnet-4-6"        # current default
max_tokens = 8192

[memory]
enabled = true
embed_model = "nomic-embed-text"   # or voyage-3.5 (VOYAGE_API_KEY)

[budget]
daily_usd = 5.00
monthly_usd = 50.00

[notifications]
enabled = true
on_background_done = true
on_autotest_fail = true
on_budget = true

[native_tools]
web_search = false
code_execution = false
x_search = false

[computer_use]
enabled = false   # DANGER: only with claude-opus-4-7+

[agent]
system_prompt = "..."

[router]
default = "anthropic"
fast_model = "anthropic:claude-haiku-4-5"
heavy_model = "anthropic:claude-opus-4-7"
embed_model = "ollama:nomic-embed-text"
fallback = ["anthropic", "xai", "openai", "ollama"]
```

## Adding a new tool

1. Create `crates/harness-tools/src/tools/mytool.rs`, implement `Tool` trait.
2. Export from `crates/harness-tools/src/tools/mod.rs`.
3. Register in `build_tools()` in `src/main.rs`.
4. Done — tool schema is automatically sent to the provider.

## Adding a new provider

1. Create `crates/harness-provider-<name>/`, implement `Provider` trait from `harness-provider-core`.
2. Add a new `build_provider` arm in `crates/harness-provider-router/src/lib.rs`.
3. Add env-key detection in the smart-defaults block of `ProviderRouter::from_config`.

## Self-dev mode

```bash
harness self-dev --src . --model claude-sonnet-4-6
```

The agent gets `RebuildSelfTool` and `ReloadSelfTool`. Workflow: read source → edit → `rebuild_self` (check_only=true for fast check) → fix errors → `rebuild_self` → `reload_self`.
On Unix, `reload_self` calls `exec()` to hot-swap the binary in-place.
