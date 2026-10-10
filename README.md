# Harness

[![CI](https://github.com/seanebones-lang/harness/actions/workflows/ci.yml/badge.svg)](https://github.com/seanebones-lang/harness/actions/workflows/ci.yml)
[![Coverage](https://github.com/seanebones-lang/harness/actions/workflows/coverage.yml/badge.svg)](https://github.com/seanebones-lang/harness/actions/workflows/coverage.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

Harness is Sean McDonnell's personal work assistant, written in Rust. It reads and edits repositories, runs tools, keeps resumable sessions, and offers a terminal interface and a local web interface. Persistent workflows cover software, research, documents, websites, automation, and Apple projects. The repository is public and [MIT licensed](LICENSE). Development is focused on dependable personal use.

You choose the exact `provider:model` route. The first entry is primary; optional fallbacks run in your chosen order. Credentials never rank providers or insert a fallback. Built-in provider names are presets, and custom OpenAI-compatible endpoints can be configured without adding Rust code.

**Current status:** version 1.3.0, under active maintenance. Build and verify the current source before use; a successful build does not establish every optional integration or operating system. [The assessment and acceptance record](docs/PERSONAL_USE.md) distinguishes verified workflows from remaining checks. Earlier competition, promotion, and release notes are historical.

## Build and verify

Install Git and Rust through [rustup](https://rustup.rs). The repository pins Rust 1.95.0.

```sh
git clone https://github.com/seanebones-lang/harness.git
cd harness
cargo build --locked
./target/debug/harness --version
./target/debug/harness --help
make verify
```

`make verify` runs workspace formatting, strict linting, tests, and synthetic agent/HTTP acceptance. Its tests do not require API keys. For an optimized binary:

```sh
cargo build --locked --profile release-lto
mkdir -p ~/.local/bin
install -m 755 target/release-lto/harness ~/.local/bin/harness
```

On Windows, copy `target\release-lto\harness.exe` into a directory on your user PATH. The [installation guide](docs/INSTALL.md) covers platform tooling and installer scripts. Prebuilt releases may lag source; use the freshly built executable when checking new behavior.

## First use

Make the chosen provider's credential available through its documented environment variable, or run a local backend such as Ollama. Harness setup stores environment-variable names rather than new secret values.

```sh
cd /path/to/your/project
harness setup
harness route show
harness doctor
harness
```

Select your own provider and model in setup. An explicit ordered route can also be saved with `harness route set <provider>:<model> [...]`. Run `harness route --help` for scope and custom-endpoint options. [Provider configuration](docs/PROVIDERS_OPENAI_COMPAT.md) explains compatible endpoints; [the user manual](Start%20Here/USER%20MANUAL.md) covers first use.

```sh
harness "explain this repository"
harness --plan "refactor this function and show the proposed changes"
harness --resume <session-id> "continue"
harness sessions
harness export <session-id>
harness cost today
```

A failed or token-limited one-shot run exits with an error and saves its session for inspection or resume. Session exports retain full tool results. Tool execution limits stop an incomplete run rather than claiming completion.

## Build a backend, RAG service, or automation

Start with an outcome and observable acceptance criteria:

```sh
cd /path/to/your/project
harness build "Build a local document retrieval API in Python" \
  --accept "Ingested documents retain source identifiers" \
  --accept "Answers cite retrieved sources; empty retrieval is explicit" \
  --accept "The API starts locally and handles malformed requests" \
  --check "python3 -m pytest"
```

This saves an editable `.harness/build.toml` and runs the existing agent. It discovers verification commands from existing root manifests when none are supplied. Use `--prepare-only` to review the brief without API calls. Repeating `--accept` or `--check` supplies the respective list; omitted lists keep existing values when continuing the same outcome. A different outcome archives the prior brief and progress notes.

Run `harness build` to continue the saved outcome, or `harness --resume <session-id> build` to also reuse a conversation. Build runs save their session ID, turn status, and recent shell/test output in `.harness/BUILD_PROGRESS.md`, even when the model omits notes or the run fails. The terminal and HTTP agent load the same brief and progress alongside project instructions. Recheck those observations against the workspace; a finished turn does not certify acceptance. The brief itself does not prove completion or execute checks; the agent must run them and report their evidence. Supply checks appropriate to your repository and review any detected commands.

## Use it across your work

Choose an existing project or work folder with `-C`, then describe the outcome:

```sh
harness -C "/path/to/work" work --kind research "Compare these sources and save a cited report"
harness -C "/path/to/work" work --kind documents "Revise this proposal and verify its figures"
harness -C "/path/to/site" work --kind website "Fix booking validation and verify the phone layout"
harness -C "/path/to/job" work --kind automation "Add a dry run and prevent duplicate processing"
harness -C "/path/to/app" work --kind apple "Fix the failing simulator build"
harness -C "/path/to/backend" work --kind software "Add a sourced retrieval endpoint"
```

`work` uses the same runner, editable brief, observed receipts, and session recovery as `build`. Use `harness -C "/path/to/work" work` to continue its saved kind and outcome. Add `--prepare-only` for a local brief with no provider calls; `--accept` and `--check` customize its criteria and commands. Old build briefs still load as software. Changing the kind requires an explicit outcome and archives earlier progress. Research and document briefs do not automatically detect repository test commands.

Preparation installs the matching bundled `SKILL.md` and any helper into `.agents/skills/`. CLI, terminal, and HTTP context include skill names, descriptions, and paths; the agent reads matching instructions and supporting files only as needed. Existing skills are preserved. `harness skills list` reports bundled workflows, project skills, and invalid or duplicate metadata; `harness skills install documents` installs a workflow independently of a brief.

You can place a reviewed skill folder from [OpenAI's skills repository](https://github.com/openai/skills) or your own workflow in `.agents/skills/<name>/SKILL.md` (or `.harness/skills/`). Harness reads YAML `name` and `description`; it does not implement upstream plugin metadata, connectors, tool grants, or installer behavior. Supporting paths resolve relative to the skill folder. Keep the source skill's license with imported files and verify its tools and dependencies before using it. Project skills follow the existing file/shell approval boundaries and are limited to the workspace; external symlinks are rejected during discovery and installation.

The document helper can extract bounded, source-labeled text from DOCX, XLSX, PPTX, CSV/TSV, Markdown, and text with Python 3. PDF extraction needs Poppler's `pdftotext`. It labels spreadsheet formula results as cached, reports truncation, and never implies layout review or recalculation:

```sh
python3 .agents/skills/documents/scripts/extract.py --probe
python3 .agents/skills/documents/scripts/extract.py "input.xlsx"
```

Formatted document creation uses appropriate optional libraries; the dependency probe reports common packages without installing them. Browser review needs configured browser tooling and a model capable of inspecting images. Research uses the search tools actually loaded for the selected provider or MCP connection; otherwise it works from supplied sources or known URLs and reports that boundary. A workflow supplies guidance and tested local mechanics; completion remains dependent on the model and configured integrations.

Attach a screenshot or rendered page to a one-shot request, `run`, `build`, or `work` with `--image`:

```sh
harness -C "/path/to/site" work --kind website "Review this phone layout" --image "preview.png"
```

The OpenAI-compatible transport sends image bytes as multipart user content and retains them in the saved session. Native Anthropic, xAI, Ollama, and Bedrock adapters currently reject image attachments explicitly rather than dropping them. PNG, JPEG, GIF, and WebP attachments must be nonempty and at most 10 MiB. Choose a route that supports image input; transport acceptance does not prove a model interpreted or visually validated the image.

## Useful capabilities

- **Repository tools:** reading, writing, patches, search, shell commands, Git, GitHub CLI, and test execution.
- **Sessions and memory:** SQLite conversation storage, session export, project instructions, optional semantic recall and ambient consolidation.
- **Workers:** tracked swarm tasks, cancellation, cleanup, wall-time limits, tool allowlists, and explicit worker model routes.
- **Interfaces:** terminal UI, one-shot commands, local HTTP/SSE server, and a daemon. VS Code and Tauri clients are optional.
- **Interop:** MCP tools/resources/sampling and LSP lookups. Browser, computer use, database, notebook, Docker, voice, and platform bridges are optional integrations with their own setup.
- **Deadbolt:** an out-of-band control for agent tool/MCP/spawn authority. See [its scope and commands](docs/DEADBOLT.md).

Use optional integrations when they solve a real task. They are not requirements for the core coding workflow.

## Local web interface

```sh
harness serve --addr 127.0.0.1:8787
```

Open the address printed by the server. API requests require its bearer token; keep the service on loopback for personal use. [Container instructions](docs/CONTAINERS.md) cover a separate runtime environment.

## Trust and recovery

Tools act with the authority of your account. Filesystem restrictions, plan approvals, and Deadbolt controls do not establish arbitrary-code isolation for a shell command. Review the [trust boundaries](docs/THREAT_MODEL.md), use plan mode for changes you want to inspect, and keep work in version control. Cancellation and timeouts are part of the agent's runtime contract.

Credentials and personal state belong outside the public repository. Local `.env` files are ignored. The automated acceptance scripts use isolated temporary homes and synthetic providers.

## Development

[The current backlog](docs/CTO_BACKLOG.md) orders the remaining work. [The developer map](CLAUDE.md) explains the modules, and [the architecture](ARCHITECTURE.md) describes the existing design. [The cookbook](docs/COOKBOOK.md) and [keyboard reference](docs/SHORTCUTS.md) cover detailed workflows. Coverage measurements in [COVERAGE.md](COVERAGE.md) are dated evidence; the workflow badge reflects CI status.

```sh
make verify
make web-test
make supply-chain
```

Provider changes should include mock transport tests for split network chunks, terminal conditions, and tool-result continuation. Preserve exact user-selected routes. OpenAI's [Codex repository](https://github.com/openai/codex), [Agents SDK](https://github.com/openai/openai-agents-python), and [Python SDK](https://github.com/openai/openai-python) are implementation references; their licenses remain separate, and Harness does not imply complete API or feature parity.

Contributions follow the [MIT license and contribution guide](CONTRIBUTING.md).
