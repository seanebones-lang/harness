# harness — Codebase Guide

Rust coding agent harness powered by Grok (xAI). Fast, low-memory, multi-agent.

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
# Interactive TUI
XAI_API_KEY=xai-... harness

# One-shot
XAI_API_KEY=xai-... harness "refactor src/agent.rs to use a state machine"

# Resume a session
harness --resume abc12345 "continue where we left off"

# HTTP server
harness serve --addr 127.0.0.1:8787

# Connect to server
harness connect http://127.0.0.1:8787 "hello"

# List sessions
harness sessions

# Self-development mode (agent edits itself)
harness self-dev --src . --model grok-3
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
│   └── config.rs                   TOML config structs
├── crates/
│   ├── harness-provider-core/      Provider trait, Message/Delta/Tool types
│   ├── harness-provider-xai/       Grok streaming SSE client + embed API
│   ├── harness-tools/              Tool trait, ToolRegistry, ToolExecutor + built-ins
│   ├── harness-memory/             SQLite session store + vector memory store
│   └── harness-mcp/                MCP stdio protocol client + tool adapter
├── config/default.toml             Annotated default configuration
├── tests/smoke_test.rs             12 integration tests (no API key required)
└── scripts/install.sh              Install from source to ~/.local/bin
```

## Key types

### `harness-provider-core`

| Type | Description |
|---|---|
| `Provider` trait | `async fn stream_chat(req: ChatRequest) -> DeltaStream` |
| `Message` | `{role: Role, content: MessageContent, tool_call_id?}` |
| `Delta` | `Text(String)` \| `ToolCall(ToolCall)` \| `Done{stop_reason}` |
| `ToolDefinition` | OpenAI-format function schema (Grok accepts natively) |
| `ChatRequest` | Builder: `.with_messages()`, `.with_tools()`, `.with_system()` |

### `harness-provider-xai`

`XaiProvider` implements `Provider`. Also exposes `embed(model, text) -> Vec<f32>` for the `/embeddings` endpoint.
`XaiConfig::new(api_key).with_model("grok-3-fast").with_max_tokens(8192)`
Base URL: `https://api.x.ai/v1` — OpenAI-compatible.

### `harness-tools`

`Tool` trait: `fn definition() -> ToolDefinition` + `async fn execute(args: Value) -> Result<String>`.

Built-in tools:
- `ReadFileTool`, `WriteFileTool`, `ListDirTool`
- `PatchFileTool` — surgical old→new text replacement with diff output
- `ShellTool` — runs `sh -c <command>`, configurable timeout
- `SearchCodeTool` — regex over gitignore-aware file walk
- `SpawnAgentTool` — runs a sub-agent with base tools only
- `RebuildSelfTool` — `cargo build --profile selfdev` in source dir
- `ReloadSelfTool` — `exec()` the new binary on Unix (hot-reload)

### `harness-memory`

`SessionStore` — SQLite WAL, sessions table. `save/load/find(prefix-or-name)/list`.
`MemoryStore` — memories table with JSON float embeddings. `insert/search(cosine-similarity, top-k)`.
Both wrap `Arc<Mutex<Connection>>` so they're `Clone`.

### `harness-mcp`

`McpClient::spawn(name, config)` — forks process, runs initialize handshake, exposes `list_tools()` and `call_tool()`.
`load_mcp_tools(path, registry)` — reads `mcp.json`, registers all server tools automatically.
Auto-discovers config at `.harness/mcp.json`, `.claude/mcp.json`, `~/.harness/mcp.json`.

## Agent loop (`src/agent.rs`)

```
drive_agent(provider, tools, memory?, embed_model?, session, system_prompt, events?) -> Result<()>
  │
  ├─ build_augmented_system()   embed last user msg → cosine search → inject top-3 memories
  │
  └─ loop:
       stream_chat(req) → Delta stream
         TextChunk  → emit event, buffer text
         ToolCall   → collect into pending list
         Done       → break or set stop_reason
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
│ ┌ [grok] ●  (streaming, yellow)               │  ← shell: Build succeeded │
│ │ Here's the refactored version…              │  memory: recalled 2 entries│
├─────────────────────────────────────────────┴───────────────────────────┤
│  Message: _                                                               │
├───────────────────────────────────────────────────────────────────────────┤
│  Session abc12345 · grok-3-fast · 4 turns                                 │
└───────────────────────────────────────────────────────────────────────────┘
```

Keys: `Enter` send · `↑↓` scroll chat · `PgUp/PgDn` scroll event log · `Ctrl+C` quit.

## HTTP API (`src/server.rs`)

```
POST /api/chat          body: {prompt, session_id?}   → SSE AgentEvent stream
GET  /api/sessions      → [{id, name, updated_at}]
GET  /api/sessions/:id  → full Session JSON
GET  /api/health        → {status, model}
```

SSE event types: `text_chunk`, `tool_start`, `tool_result`, `memory_recall`,
`sub_agent_spawned`, `sub_agent_done`, `done`, `error`.

## Configuration (`~/.harness/config.toml`)

```toml
[provider]
api_key = "xai-..."      # or XAI_API_KEY env var
model = "grok-3-fast"    # grok-3 | grok-3-fast | grok-3-mini | grok-3-mini-fast
max_tokens = 8192
temperature = 0.7

[memory]
enabled = true
embed_model = "grok-3-embed-english"

[agent]
system_prompt = "..."

[mcp]
config_path = ".harness/mcp.json"   # optional path override

[browser]
enabled = false                      # or pass --browser at runtime
url = "http://localhost:9222"        # Chrome --remote-debugging-port
```

## MCP server config (`.harness/mcp.json`)

```json
{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user"],
      "env": {}
    }
  }
}
```

## Adding a new tool

1. Create `crates/harness-tools/src/tools/mytool.rs`, implement `Tool` trait.
2. Export from `crates/harness-tools/src/tools/mod.rs`.
3. Register in `build_tools()` in `src/main.rs`.
4. That's it — the tool schema is automatically sent to Grok.

## Adding a new provider

1. Create `crates/harness-provider-<name>/`, implement `Provider` trait from `harness-provider-core`.
2. Add a new variant to `config::ProviderConfig`.
3. Select it in `main.rs` based on config.

## Self-dev mode

`harness self-dev --src . --model grok-3`

The agent gets `RebuildSelfTool` and `ReloadSelfTool` in addition to all base tools.
Workflow: read source → edit → `rebuild_self` (check_only=true for fast check) → fix errors → `rebuild_self` → `reload_self`.
On Unix, `reload_self` calls `exec()` to hot-swap the binary in-place.
