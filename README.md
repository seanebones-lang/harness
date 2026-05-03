# Harness — Rust Coding Agent (May 2026)

Harness is a terminal-based AI coding assistant. It reads files, edits code, runs shell commands, searches your codebase, manages sessions with semantic memory, and can spawn sub-agents for parallel tasks.

Default model: **claude-sonnet-4-6** (Anthropic). Falls back to xAI → OpenAI → local Ollama based on which API keys are set.

**Status:** Beta — fine for daily use; expect ongoing polish. Before tagging a release, run the gates in [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md). Latest go/no-go notes: [`docs/RELEASE_STATUS.md`](docs/RELEASE_STATUS.md).

**Plain-language guide:** [`Start Here/USER MANUAL.md`](Start%20Here/USER%20MANUAL.md) — complements this README with the same first-run story.

**Full installation (every OS, FAQ, troubleshooting):** [`docs/INSTALL.md`](docs/INSTALL.md)

## Prerequisites

- **Rust** (stable, edition 2021) via [rustup](https://rustup.rs) — on Windows, use the **MSVC** toolchain (Visual Studio C++ build tools) unless you know you need GNU.
- **Git** — required to clone the repo (and **Git for Windows** is recommended on Windows so `sh.exe` is on `PATH` for the `shell` tool’s POSIX behavior).
- **Platforms:** **macOS**, **Linux**, and **Windows** are all exercised in [CI](.github/workflows/ci.yml) (`fmt`, `clippy --all-features`, `test`, `build`). Optional features (voice, computer-use, desktop notifications) vary by OS — see **Optional features by platform** below.

---

## Quick Start (macOS / Linux)

```bash
# 1. Build and install (replace clone URL if you use a fork)
git clone https://github.com/seanebones-lang/harness.git
cd harness
cargo build --profile release-lto
install -m 755 target/release-lto/harness ~/.local/bin/harness
export PATH="$HOME/.local/bin:$PATH"   # add to ~/.zshrc permanently

# 2. Set your API key (any of these work)
export ANTHROPIC_API_KEY="sk-ant-..."   # preferred — unlocks prompt caching + thinking
export XAI_API_KEY="xai-..."            # fallback
export OPENAI_API_KEY="sk-..."          # fallback

# 3. Run from any project (optional: run `harness init` once first — seeds ~/.harness/config.toml)
cd /path/to/your/project
harness
```

**Alternative — install script (macOS / Linux):** from the repo root after clone, you can use [`scripts/install.sh`](scripts/install.sh) (sets `HARNESS_INSTALL_DIR` if you want a non-default bin dir — see script header). Review any `curl | bash` one-liner before running.

Prebuilt binaries for tagged releases may be attached as artifacts on [GitHub Releases](https://github.com/seanebones-lang/harness/releases) (see `.github/workflows/release.yml`). Prefer building from source or CI-verified `main` for the latest fixes.

### Quick Start (Windows, PowerShell)

From a directory where you want the source (or use an existing clone and `cd` into it):

```powershell
# Clone (skip if you already have the repo)
git clone https://github.com/seanebones-lang/harness.git
cd harness

cargo build --profile release-lto
New-Item -ItemType Directory -Force -Path "$HOME\.local\bin" | Out-Null
Copy-Item -Force .\target\release-lto\harness.exe "$HOME\.local\bin\harness.exe"
# Add %USERPROFILE%\.local\bin to your User PATH, then open a new terminal.

$env:ANTHROPIC_API_KEY = "sk-ant-..."   # or XAI_API_KEY / OPENAI_API_KEY
cd C:\path\to\your\project
harness
```

Or run the installer script: `.\scripts\install.ps1` (from a clone) or download raw `install.ps1` from the repo and execute in PowerShell.

### Quality gates (match CI)

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo build --profile release-lto
```

That's it. Harness auto-detects which API keys are set and picks the best available provider. Run **`harness init`** once if you want a generated global config under `~/.harness/` (install scripts may already create `config.toml`).

### Development snapshot

- **`TODO.md`** — remaining work is mostly **Polish** (ambient abstraction, browser/ambient test coverage, session list timing). Older Critical/Important backlog items are **implemented** on current `main`.
- **CI:** Pull requests and `main` run **fmt**, **clippy `--all-features`**, **tests**, **build**, and **install-script smoke jobs** (`scripts/install.sh` on Ubuntu + macOS, `scripts/install.ps1` on Windows) — see [`.github/workflows/ci.yml`](.github/workflows/ci.yml). Tag **GitHub Releases** binaries are produced by [`.github/workflows/release.yml`](.github/workflows/release.yml); ship only when `main` is green and [`docs/PUBLIC_RELEASE.md`](docs/PUBLIC_RELEASE.md) is satisfied.

See [`CLAUDE.md`](CLAUDE.md) for module-level detail and contributor hooks (`core.hooksPath`).

---

## May 2026 Model Lineup

| Provider  | Default model                    | Fast model                   | Heavy model          |
|-----------|----------------------------------|------------------------------|----------------------|
| Anthropic | `claude-sonnet-4-6` ($3/$15/M)   | `claude-haiku-4-5` ($1/$5/M) | `claude-opus-4-7` ($5/$25/M, thinking) |
| xAI       | `grok-4.3` ($1.25/$2.50/M) | `grok-4-1-fast-reasoning` ($0.20/$0.50/M) | same |
| OpenAI    | `gpt-5.5` ($5/$30/M)             | `gpt-5.4-mini` ($0.75/$4.50/M) | same |
| Ollama    | `qwen3-coder:30b` (local)        | same                         | same                 |

Switch models interactively:
```bash
harness models                                 # list all models
harness models --set anthropic:claude-opus-4-7 # switch to Opus for a project
harness models --set xai:grok-4.3              # xAI default flagship
```

---

## Daily workflow

```bash
cd my-project
harness                          # start a new session (TUI)
harness --resume abc12345        # continue a previous session
harness "explain what main.rs does"   # one-shot, no TUI
harness --plan                   # approve-mode: preview changes before they apply
harness --think 10000            # enable extended thinking with 10k token budget
harness status                   # show config, API key, recent sessions
harness doctor                   # health checks (keys, daemon, MCP paths, …)
harness completions zsh          # print completions → save to your shell's completion dir
harness swarm list               # parallel sub-agent tasks
harness swarm status <task-id>   # one task
harness swarm result <task-id>   # output when done
harness trace                    # list spans from last local trace (needs observability)
```

---

## TUI keybindings

Full cheat sheet: [`docs/SHORTCUTS.md`](docs/SHORTCUTS.md). Phase E highlights:

| Key       | Action                             |
|-----------|------------------------------------|
| `Enter`   | Send message                       |
| `Shift+Enter` / `Alt+Enter` | Newline in input           |
| `↑ / ↓`   | Scroll chat / history navigation   |
| `PgUp/Dn` | Scroll event log (right panel)     |
| `Ctrl+S`  | Voice record (Whisper) — **Phase E** (was Ctrl+V in Phase D) |
| `Ctrl+Y`  | Copy last assistant reply          |
| `Ctrl+F`  | Search chat                        |
| `Ctrl+L`  | Jump chat to bottom                |
| `Ctrl+]` / `Ctrl+[` | Resize right panel           |
| `Ctrl+E`  | Fork mode — branch from a past turn |
| `Ctrl+C`  | Quit                               |

### Slash commands

| Command              | Effect                                      |
|----------------------|---------------------------------------------|
| `/think [N]`         | Enable extended thinking (N = token budget) |
| `/remember t: fact`  | Store fact under topic t in `.harness/memory/` |
| `/forget t`          | Delete memory topic t                       |
| `/memories`          | List all memory topics                      |
| `/pr [N]`            | List open PRs or load PR #N for review      |
| `/issues`            | List open GitHub issues                     |
| `/ci`                | Show recent CI workflow runs                |
| `/notify test`       | Send a test desktop notification            |
| `/cost`              | Show token usage + cost estimate            |
| `/model X`           | Switch model mid-session                    |
| `/runs`              | List background runs                        |
| `/focus [N]`         | Pomodoro: silence notifications N min (default 25) |
| `/schema …`          | Strict JSON output schema (see docs)        |
| `/obsidian save`     | Save reply to Obsidian (when configured)    |
| `/help`              | Full command list                           |

---

## Optional features by platform

| Feature | macOS | Linux | Windows | Notes |
|--------|-------|-------|---------|--------|
| **Core CLI / TUI** | Yes | Yes | Yes | Same `harness` binary; CI covers all three. |
| **`shell` tool** | `sh -c` | `sh -c` | Git `sh`/`bash` if on `PATH`; else **`cmd.exe /C`** (limited POSIX) | Install **Git for Windows** and ensure `usr\bin` is on `PATH` for best results. |
| **GitHub `/pr`, `/issues`, `harness pr`** | With `gh` | With `gh` | With `gh` | Run `gh auth login` once. |
| **Desktop notifications** | Notification Center | **libnotify** (e.g. `libnotify-bin`) | Varies / may be limited | See [`config/default.toml`](config/default.toml). |
| **Voice (`harness voice`, Ctrl+S)** | `sox` / `afrecord` + Whisper or `whisper-cli` | `sox rec` + backends | Not first-class | Prefer OpenAI Whisper API or local tooling you already use. |
| **Computer use** | **`cliclick`** (`brew install cliclick`) | **`xdotool`** | **Not supported** | Opus 4.7+ only; dangerous — see [`config/default.toml`](config/default.toml). |
| **VS Code extension** | Yes | Yes | **Unix socket** — use **WSL** or wait for a Windows transport | Default socket `~/.harness/daemon.sock`. |

---

## Per-project setup

```bash
cd my-project
harness init --project           # writes .harness/config.toml
harness                          # agent is now tuned for this repo
```

### Project memory

Store persistent facts about your project — they're injected into every session:

```bash
harness memorize architecture "Monorepo: crates/ + src/. Provider abstraction in harness-provider-core."
harness memorize tests "Run cargo test --workspace. Lints via cargo clippy."
harness memories                 # list all topics
harness forget architecture      # delete a topic
```

### Project lifecycle commands

Manage linked repos from one command surface:

```bash
harness proj ls                              # alias: harness project list
harness project dashboard                    # all projects: branch, ahead/behind, dirty state
harness project status my-project            # one project health view
harness project sync my-project              # fetch + ff-only pull
harness project sync --all                   # bulk sync every linked project
harness project push my-project              # push current branch (safe defaults)
harness project exec my-project -- cargo test
```

Create/publish flows:

```bash
harness project init my-new-app              # mkdir + git init + auto-link
harness project publish my-new-app --private # gh repo create + remote wiring
harness project clone git@github.com:you/repo.git
harness project import --root ~/code --recursive
harness project prune                         # drop missing paths from registry
```

Helpful aliases:
- `harness proj ...` = `harness project ...`
- `ls`, `dash`, `st`, `up`, `pub`, `run`, `ship`, `rm`, `scan`, `clean`, `new`, `link`, `cl`

### Quickstart recipes

1) **Start a brand-new app**

```bash
harness project init my-new-app
harness project exec my-new-app -- git add .
harness project exec my-new-app -- git commit -m "chore: initial scaffold"
harness project publish my-new-app --private --push
```

2) **Onboard existing local repos**

```bash
harness project import --root ~/code --recursive
harness project dashboard
harness project status some-repo
```

3) **Daily update + push loop**

```bash
harness proj up --all              # pull latest safely for every linked repo
harness project dashboard          # check what needs attention
harness project exec my-app -- npm test
harness project pub my-app         # alias for `project push`
```

---

## Session management

```bash
harness sessions                 # list recent sessions
harness --resume <id> "continue" # resume by id prefix or name
harness export <id>              # print session as Markdown
harness delete <id>
```

---

## GitHub workflow

```bash
harness pr 123                   # load PR #123 diff + comments into agent session
harness pr 123 --comment "LGTM"  # post a review comment
```

Requires `gh` CLI installed and authenticated (`gh auth login`).

---

## Cost tracking

```bash
harness cost today               # today's spend
harness cost week                # last 7 days
harness cost by-model            # breakdown by model
harness cost watch               # live tail (refresh every 5s)
```

Streaming chat counts toward the ledger whenever the backend supplies usage (**xAI** requests `stream_options.include_usage`; Anthropic emits usage deltas similarly). Wallet math still depends on your provider’s streamed fields and local pricing tables.

Set budget limits in `~/.harness/config.toml`:
```toml
[budget]
daily_usd = 5.00
monthly_usd = 50.00
```

Status bar turns yellow at 80%, red at 100%. Desktop notification fires at each threshold.

---

## Cross-machine sync

Encrypt and sync `~/.harness` state (sessions, memory, cost) to a private git repo:

```bash
harness sync init git@github.com:you/harness-state.git
harness sync push                # encrypt + push
harness sync pull                # pull + decrypt (on another machine)
harness sync status              # show recent syncs
```

Passphrase storage: **macOS** can use Keychain; **Linux and Windows** typically use the file fallback `~/.harness/.sync-key` (mode `0600` on Unix) — see sync code paths in the repo and [`CLAUDE.md`](CLAUDE.md).

---

## Voice input

```bash
harness voice                    # record one-shot, print transcript
harness voice --send             # record + send to agent immediately
harness voice --realtime         # OpenAI Realtime API duplex (requires OPENAI_API_KEY)
```

In the TUI use **`Ctrl+S`** for push-to-talk style recording (see [`docs/MIGRATION.md`](docs/MIGRATION.md) if you still use Ctrl+V).

OS capture backends and caveats: see **Optional features by platform**. Summarized: **OpenAI Whisper** (`OPENAI_API_KEY`) uses `gpt-4o-transcribe`; **local** `whisper-cli` uses whisper.cpp where installed.

---

## Computer use (opt-in, dangerous)

Enable in `~/.harness/config.toml` or `.harness/config.toml`:
```toml
[computer_use]
enabled = true   # DANGER: agent can control mouse/keyboard
```

**Platforms:** **macOS** needs **`cliclick`**. **Linux** needs **`xdotool`**. **Windows** is not supported for computer use today. Requires **Claude Opus 4.7+**. The TUI shows `[COMPUTER USE LIVE]` when active. See [`config/default.toml`](config/default.toml) for comments.

---

## Web UI (optional)

```bash
harness serve --addr 127.0.0.1:8787
# then open http://127.0.0.1:8787
```

The bundled page keeps the chat **session ID in localStorage** across reloads and includes a **New session** control to start fresh.

---

## Browser tool (Chrome CDP, optional)

Requires Chrome/Chromium launched with **`--remote-debugging-port`** (typically `9222`). Enable `[browser]` in config or pass **`--browser`** at startup; optionally set **`--browser-url`**. Registers a `browser` tool (navigate, screenshot, DOM actions). Full detail: **`harness-browser`** in [`CLAUDE.md`](CLAUDE.md).

---

## Desktop app (Tauri 2, optional)

Wraps the same UI in a native window with tray icon and **Cmd+Shift+H** (Ctrl+Shift+H on Linux/Windows) to show/hide.

```bash
cd apps/desktop
npm install
npm run dev      # development
npm run build    # release bundle
```

Requires `harness` on `PATH` for auto-spawn of `harness daemon`. See [`apps/desktop/README.md`](apps/desktop/README.md).

---

## VS Code extension (optional)

`extensions/vscode/` — side-panel chat against the harness daemon over a **Unix domain socket** (`~/.harness/daemon.sock` by default). **Windows:** use **WSL** for a supported setup today, or run the TUI / `harness serve` natively. Install with `npm install`, then **Run Extension** or package with `vsce`.

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

---

## Configuration

Config is loaded from `.harness/config.toml` (project) then `~/.harness/config.toml` (global).
Full annotated reference: [`config/default.toml`](config/default.toml).

Key settings:

```toml
[provider]
api_key = "sk-ant-..."          # or set ANTHROPIC_API_KEY env var
model = "claude-sonnet-4-6"     # default model

[budget]
daily_usd = 5.00
monthly_usd = 50.00

[notifications]
enabled = true

[native_tools]
web_search = true               # provider-native web search

[browser]
enabled = false                 # or use CLI --browser ; needs Chrome remote debugging port

[computer_use]
enabled = false                 # DANGER when true

[memory]
enabled = true

[agent]
system_prompt = "..."           # customize agent persona

# Optional blocks — copy from `config/default.toml` verbatim. Active examples include `[router]`;
# additional tables (`observability`, `swarm`, …) ship commented — uncomment matching entries only.
```

---

## Architecture

```
src/main.rs              CLI, subcommands, tool wiring
src/agent.rs             core agentic loop + memory injection
src/tui.rs               ratatui two-panel TUI
src/server.rs            axum HTTP/SSE server
src/cost_db.rs           SQLite cost tracking
src/memory_project.rs    .harness/memory/ project facts
src/sync.rs              age-encrypted cross-machine sync
src/notifications.rs     desktop notification hooks
crates/
  harness-provider-anthropic/   Claude Sonnet/Opus/Haiku + prompt caching
  harness-provider-openai/      GPT-5.x
  harness-provider-xai/         Grok 4.x
  harness-provider-ollama/      Local Ollama (Qwen3-Coder)
  harness-provider-router/      Smart multi-provider router with env-key detection
  harness-provider-core/        Shared types (ChatRequest, Delta, Provider trait)
  harness-tools/                Tool trait + shell/gh/computer/file/search/patch/spawn
  harness-memory/               SQLite session store + vector memory
  harness-mcp/                  MCP stdio protocol client
  harness-browser/              Chrome CDP browser tool
  harness-voice/                Whisper audio transcription + Realtime API duplex
  harness-term-graphics/        Inline terminal images (Kitty / iTerm2 / Sixel)
extensions/vscode/             VS Code integration (MVP)
apps/desktop/                  Tauri 2 native shell
```

For a developer deep-dive see [`CLAUDE.md`](CLAUDE.md). User-facing migration notes: [`docs/MIGRATION.md`](docs/MIGRATION.md).

---

## Troubleshooting

| Symptom | What to try |
|--------|--------------|
| `command not found: harness` | **Unix:** add `~/.local/bin` to `PATH` (`export PATH="$HOME/.local/bin:$PATH"`). Run `hash -r` or open a new shell. **Windows:** add `%USERPROFILE%\.local\bin` to User **Path** and open a new terminal. |
| API / auth errors | **Unix:** `export ANTHROPIC_API_KEY=…`. **Windows:** `$env:ANTHROPIC_API_KEY='…'`. Run `harness status` and `harness doctor`. |
| `shell` tool behaves oddly on Windows | Install **Git for Windows** so `sh.exe` is on `PATH`; without it Harness falls back to `cmd.exe` (not POSIX). |
| `/pr`, `/issues`, `/ci` fail | Install GitHub CLI on your OS: `gh auth login`, then `gh auth status`. |
| Clippy fails locally but CI passes | Run the same command as CI: `cargo clippy --all-targets --all-features -- -D warnings`. |
| Checkpoint / `/undo` says not a git repo | Run `git init` in the project root (Harness uses git for checkpoints). |
| Web UI empty or connection errors | Start the server: `harness serve --addr 127.0.0.1:8787`, then open the URL it prints. |
| Browser / CDP tool errors | Chrome must run with `--remote-debugging-port` matching `[browser].url` in config (see `config/default.toml`). |

---

## Known limitations

Non-exhaustive list; details live in [`TODO.md`](TODO.md):

- **Polish:** ambient provider abstraction, extra `harness-browser` tests, optional ambient consolidation tests.
- **UX:** session titles from async auto-naming can lag the first `harness sessions` list right after save.
- **`shell` on Windows:** prefers Git `sh`/`bash`; without them commands run via `cmd.exe` (not POSIX).

---

## Reporting issues

Open an issue on the project’s GitHub tracker with: OS, Rust version (`rustc --version`), `harness --version`, the command you ran, and redacted logs if any.

---

## License

This project is licensed under the **MIT License** — see [`LICENSE`](LICENSE).
