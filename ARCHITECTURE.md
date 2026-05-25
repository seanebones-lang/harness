# Harness Architecture Guide

This document explains how Harness is organized for developers who want to understand, extend, or embed the system. For AI-assistant-oriented detail (type signatures, config keys, module paths), see `CLAUDE.md`. For user-facing documentation, see `README.md`.

---

## What the Binary Does

`harness` is a single statically-linked binary that acts as a terminal coding assistant. It receives user input (via an interactive TUI, one-shot CLI arguments, or an HTTP/SSE API), sends it to an LLM provider, executes any tool calls the model requests, and streams the results back to the user. State — conversation history, vector memory, cost records — persists to SQLite databases under `~/.harness/`.

---

## Provider Abstraction

The central design abstraction is the `Provider` trait in `harness-provider-core`:

```rust
trait Provider: Send + Sync {
    async fn stream_chat(&self, req: ChatRequest) -> DeltaStream;
}
```

Every LLM backend implements this trait. The agent loop calls only `stream_chat` and never knows which backend is active. `ChatRequest` carries messages, tool definitions, the system prompt, and optional flags (thinking budget, native web search, response schema). `DeltaStream` is an async stream of `Delta` values: text chunks, tool calls, usage events, and a terminal `Done`.

This design means that adding a new provider requires no changes to the agent loop, TUI, server, or tool system.

---

## Crate Map

| Crate | Purpose |
|-------|---------|
| `harness-provider-core` | Provider trait, ChatRequest builder, Delta enum, Message, ResponseSchema |
| `harness-provider-anthropic` | Claude 4.x with prompt caching and extended thinking |
| `harness-provider-openai` | GPT-5.x with streaming SSE and strict JSON schema |
| `harness-provider-xai` | Grok 4.x with native tools and X search |
| `harness-provider-ollama` | Local Ollama (Qwen3-Coder 30B default) |
| `harness-provider-mlx` | Apple Silicon MLX via `mlx_lm.server` |
| `harness-provider-router` | Env-key auto-detection and priority-based routing |
| `harness-tools` | Tool trait and all built-in tools (file, shell, search, git, gh, patch, spawn) |
| `harness-memory` | SQLite session store and vector memory with cosine search |
| `harness-mcp` | Full MCP 2025-03-26 protocol client (tools, resources, sampling, roots, progress) |
| `harness-browser` | Chrome DevTools Protocol automation |
| `harness-voice` | Whisper transcription and OpenAI Realtime API duplex |
| `harness-lsp` | LSP client for editor-aware diagnostics |
| `harness-term-graphics` | Inline image rendering (Kitty, iTerm2, Sixel) |

The `src/` directory holds the binary crate, which wires together all of the above.

---

## Data Flow

```
User input (TUI / CLI / HTTP)
        │
        ▼
  src/agent.rs: drive_agent()
        │
        ├─ build_augmented_system()
        │       │
        │       ├─ embed(user_message) ──► harness-memory: cosine search ──► top-3 recalled memories
        │       └─ load .harness/memory/*.md ──► project facts
        │
        ├─ ChatRequest::new(messages + augmented_system + tool_definitions)
        │
        ▼
  harness-provider-router: route_to_provider()
        │
        ▼
  harness-provider-{anthropic,openai,xai,ollama,...}: stream_chat()
        │
        ▼
  Delta stream: Text | ToolCall | Usage | Done
        │
        ├─ Text ──► AgentEvent::TextChunk ──► TUI / SSE / stdout
        │
        ├─ ToolCall ──► harness-tools: Tool::execute()
        │                     │
        │                     ├─ ReadFileTool, WriteFileTool, PatchFileTool
        │                     ├─ ShellTool, SearchCodeTool, GitTool, GhTool
        │                     ├─ SpawnAgentTool (recursive drive_agent)
        │                     └─ McpToolAdapter ──► harness-mcp: call MCP server
        │
        ├─ Usage ──► src/cost_db.rs: record tokens + USD
        │
        └─ Done ──► session saved to harness-memory: SessionStore
```

---

## Extension Points

### Adding a Provider

1. Create `crates/harness-provider-<name>/` with a `Cargo.toml` and `src/lib.rs`.
2. Implement the `Provider` trait from `harness-provider-core`.
3. Add a `build_provider` match arm in `crates/harness-provider-router/src/lib.rs`.
4. Add env-key detection in `ProviderRouter::from_config` so the router can auto-select it.

The new provider is then available via `--model <name>:model-id` and will be picked up by the smart router if its API key is set.

### Adding a Tool

1. Create `crates/harness-tools/src/tools/mytool.rs` and implement the `Tool` trait.
2. Export it from `crates/harness-tools/src/tools/mod.rs`.
3. Instantiate and push it in `build_tools()` in `src/cli/wiring.rs`.

The tool's `ToolDefinition` (name, description, JSON Schema for arguments) is serialized and sent to the provider automatically. No other changes are required.

### Adding an MCP Server

Create `.harness/mcp.json` in your project directory:

```json
{
  "mcpServers": {
    "myserver": {
      "command": "my-mcp-server",
      "args": ["--flag"],
      "env": {}
    }
  }
}
```

`harness-mcp` will spawn the process, run the initialize handshake, and register all tools the server advertises as `McpToolAdapter` instances alongside built-in tools.

---

## Key Design Decisions

**Rust for performance and safety.** The choice of Rust eliminates a class of memory-safety bugs that are common in C/C++ agents and avoids the startup overhead of Python or JVM runtimes. The borrow checker enforces correct ownership of async resources (sessions, tool handles) at compile time rather than at runtime.

**Trait objects for provider abstraction.** `Arc<dyn Provider>` is used throughout the agent loop. This is a deliberate trade-off: monomorphization would yield slightly faster dispatch but would couple the agent loop to specific providers at compile time, making it impossible to select providers at runtime without rebuilding.

**SQLite for all persistence.** Sessions, vector memory, cost records, and the swarm task registry all use SQLite with WAL mode. SQLite is embedded (no separate server process), supports concurrent readers, survives process crashes cleanly, and can be inspected with standard tooling. The alternative — separate files or a key-value store — would require custom serialization and lack atomic multi-record updates.

**MCP for tool interoperability.** Rather than implementing a proprietary tool plugin API, Harness adopted the Model Context Protocol as its extension mechanism. This means any MCP-compliant tool server — filesystem, database, browser, code execution sandbox — works with Harness without custom integration code.

**Age encryption for sync.** Transmitting `~/.harness` state to a git remote required a simple, auditable encryption scheme. `age` was chosen over GPG because it has no key-ring infrastructure, produces binary ciphertext with a single passphrase, and has a straightforward specification. The passphrase is stored in the OS keychain where available, falling back to a mode-0600 file.
