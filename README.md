# Harness — Rust Coding Agent

Harness is a terminal-based AI coding assistant powered by Grok (xAI). It reads files, edits code, runs shell commands, searches your codebase, manages sessions with semantic memory, and can spawn sub-agents for parallel tasks.

---

## Quick Start (3 commands)

```bash
# 1. Build and install
git clone https://github.com/seanebones-lang/harness.git
cd harness
cargo build --profile release-lto
install -m 755 target/release-lto/harness ~/.local/bin/harness
export PATH="$HOME/.local/bin:$PATH"   # add to ~/.zshrc or ~/.bashrc permanently

# 2. Initialize (sets up ~/.harness/config.toml with your API key)
harness init

# 3. Run from any project
cd /path/to/your/project
harness
```

That's it. No `source .env`, no `cargo run`, no path setup needed after step 1.

---

## Daily workflow

```bash
cd my-project
harness                          # start a new session (TUI)
harness --resume abc12345        # continue a previous session
harness "explain what main.rs does"   # one-shot, no TUI
harness --plan                   # approve-mode: preview changes before they apply
harness status                   # show config, API key, recent sessions
```

---

## TUI keybindings

| Key | Action |
|---|---|
| `Enter` | Send message |
| `↑ / ↓` | Scroll chat |
| `PgUp / PgDn` | Scroll event log (right panel) |
| `Ctrl+C` | Quit |

---

## Per-project setup

```bash
cd my-project
harness init --project           # writes .harness/config.toml with a project system prompt
harness                          # agent is now tuned for this repo
```

Project config overrides `~/.harness/config.toml` — you only need to specify what changes.

---

## Session management

```bash
harness sessions                 # list recent sessions
harness --resume <id> "continue" # resume by id prefix or name
harness export <id>              # print session as Markdown
harness export <id> --output session.md
harness delete <id>
```

---

## Web UI (optional)

```bash
harness serve --addr 127.0.0.1:8787
# then open http://127.0.0.1:8787
```

Session ID is persisted in `localStorage` so it survives page refreshes. Use the "New session" button to start fresh.

---

## MCP tools

Create `.harness/mcp.json` in your project or `~/.harness/mcp.json` globally:

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

Harness auto-discovers and loads MCP servers on startup.

---

## Browser automation

```bash
# Launch Chrome with remote debugging
chrome --remote-debugging-port=9222 --headless

# Enable browser tool
harness --browser "take a screenshot of example.com"
```

Or enable permanently in `~/.harness/config.toml`:

```toml
[browser]
enabled = true
url = "http://localhost:9222"
```

---

## Configuration

Config is loaded from `.harness/config.toml` (project) then `~/.harness/config.toml` (global). Full annotated reference: [`config/default.toml`](config/default.toml).

Key settings:

```toml
[provider]
api_key = "xai-..."     # or set XAI_API_KEY env var / .env file
model = "grok-3-fast"   # grok-3 | grok-3-fast | grok-3-mini | grok-3-mini-fast

[memory]
enabled = true

[agent]
system_prompt = "..."   # customize the agent's persona and guidelines
```

---

## Architecture

```
src/main.rs          CLI, subcommands, tool wiring
src/agent.rs         core agentic loop + memory injection
src/tui.rs           ratatui two-panel TUI
src/server.rs        axum HTTP/SSE server
crates/
  harness-provider-xai/   Grok streaming + embeddings client
  harness-tools/          Tool trait + built-in tools (file/shell/search/patch/spawn)
  harness-memory/         SQLite session store + vector memory
  harness-mcp/            MCP stdio protocol client
  harness-browser/        Chrome CDP browser tool
```

For a developer-oriented deep-dive see [`CLAUDE.md`](CLAUDE.md).

---

## License

MIT
