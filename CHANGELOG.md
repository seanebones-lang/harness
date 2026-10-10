# Changelog

Current licensing is MIT. Historical entries describe the policy and results at their recorded dates.

All notable changes to NextEleven Harness will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Persistent `harness work` workflows for software, research, documents, websites, automation, and Apple projects; `-C/--directory` selects the workspace before environment/config/tool initialization
- Workspace skill discovery with YAML metadata, deferred instruction loading, offline bundled installation, validation diagnostics, and preservation of user edits
- Source-labeled Office/text extraction with explicit cached-formula, truncation, and missing-PDF-dependency reporting; actual-binary document/research integration checks
- Build runs save their session ID, turn status, and observed shell/test output even when the model omits progress notes
- Persistent `harness build` workflow with editable outcomes, acceptance criteria, existing-project check discovery, offline brief preparation, and cross-session progress context
- Personal-use acceptance record and real terminal cancellation/session recovery smoke tests
- Isolated real-binary CLI and synthetic provider/HTTP release smoke, including tool execution, exact model routing, session persistence, and export
- Installer and Homebrew checksum contract tests with failure-preservation checks
- Provider-neutral `harness route show|set|model|add|remove|move|custom` commands with explicit global/project scope
- First-class presets for Cerebras, DeepSeek, Fireworks, Groq, Hugging Face, NVIDIA, OpenRouter, Perplexity, SambaNova, and Together through the OpenAI-compatible transport
- `api_key_env` and `kind = "openai-compatible"` configuration for future bearer-authenticated endpoints without source changes

### Changed
- Default and newly initialized instructions support broader work; config scaffolds use the current built-in prompt rather than stale duplicated coding-only instructions
- Aligned first-party packaging and contribution terms with MIT; moved competition and promotion material into history
- Consolidated the active personal-use workflow and verification command
- Setup now asks for one or more exact `provider:model` entries and preserves their order; no provider or model is marked recommended
- Runtime no longer chooses a provider priority, inserts implicit fallbacks, or silently falls back to Ollama
- Browser setup edits the same exact route as the CLI and no longer collects API keys
- `harness models --set` updates only the active config; route commands provide explicit `--global` and `--project` targeting
- Removed the final xAI-specific startup branch; runtime messaging follows only the saved route
- Current product, contributor, roadmap, translated quick-start, audit, release, and competition documents now distinguish the exact user-owned route from superseded automatic-router behavior and separate current `main` evidence from tagged-release history

### Fixed
- Image attachments now reach the provider as multipart bytes and persist in sessions for positional, run, build, and work requests; unsupported command uses and oversized/empty/unsupported images fail clearly
- Malformed provider tool names remain blocked and return exact available names instead of only a confusing lease error
- Test-runner compile failures cannot report success; Cargo workspace totals accumulate across suites, and unknown counts are not presented as passing tests
- Test-runner cancellation and timeouts clean up owned Unix process groups
- UTF-8 truncation and split transport chunks, combined stream events, incomplete-response and provider-error reporting
- Failed and cancelled session recovery, complete exports, safe context compaction, and bounded cosmetic naming
- Correct fallback model selection, current OpenAI reasoning request parameters, and selected embedding models
- Compilation with default features disabled
- Ambient consolidation preserves original memories on interrupted summaries; GPT-OSS uses its actual context window
- Browser disconnect/Stop cancels backend turns; interrupted tool batches remain resumable and session-save failures are reported
- Shell cancellation and timeouts terminate the owned child and its Unix process group, preventing delayed descendant writes
- Browser startup initializes the saved resume preference before reading it; DOM regressions cover fresh profiles and authenticated streaming chat
- Release builds now use the validated tag commit, require all five native platform jobs, and stage a draft with complete checksums
- Installers reject unverified downloads and preserve version-pinned source fallbacks; Git Bash installs the correct executable filename
- Smoke scripts honor the explicitly selected binary and fail on broken offline commands
- Sync config tests use temporary paths and verify real disk round trips; strict formatting and lint restored
- Desktop lockfile synchronized; VS Code packaging is locked, audited, and includes only intended runtime files and license
- Docker uses the pinned toolchain, required embedded/build inputs, an unprivileged runtime, and excludes local state from its build context
- Homebrew refresh preserves checksum lines and updates existing values only after every artifact validates
- Windows debug/integration binaries reserve an 8 MiB main-thread stack so the expanded provider-route CLI does not overflow before `--help`, `--version`, or lightweight commands run
- `harness doctor` probes Ollama's loopback API with a 500 ms bound and checks optional tools on `PATH` instead of launching every executable serially, keeping Windows startup lightweight

### Security
- HTTP browser boundary rejects foreign loopback Host headers and cross-origin requests, with no-store responses for the local token bootstrap
- Desktop Tauri dependencies updated to remove vulnerable XML parser versions; six upstream maintenance advisories are explicitly tracked in the desktop policy
- VS Code packaging dependencies updated to remove reported npm vulnerabilities
- Unknown provider names without an explicit adapter/base URL now fail closed instead of falling through to xAI
- Setup keeps credentials in environment variables instead of writing newly entered secrets to config

### Notes
- Working tree tracks **1.3.0** on `main`. See below for the cut.

## [1.3.0] - 2026-08-09

Public **proof-of-concept** cut. Repository is visible on GitHub for evaluation only.

**License: proprietary NextEleven LLC — NOT MIT, NOT open source.**

### Added
- Gemini + Bedrock providers; Database/Notebook/Docker tools (config-gated)
- `harness bench` offline pack; swarm worker allowlist + wall timeout; model on swarm JSON
- `src/agent/*` and `src/server/*` module splits
- Threat model v2; docs refresh waves
- Swarm-50 / Swarm-51 coverage climbs; CI line gate ≥60% **met** (measured **61.65%**)
- Trust path-inject (`load_from_path` / `save_to_path`) + wiring pure edges
- Connect CLI `--url` clap fix; single-panel Hermes-style TUI
- Public-repo hardening: path scrub, secret hygiene docs, proprietary packaging labels

### Changed
- Workspace version **0.1.2-beta → 1.3.0** (binary, desktop, VS Code, Docker, Homebrew formula)
- Ship branch **main** only (`dev` folded and removed)
- LICENSE: proprietary NextEleven LLC notice (public = POC visibility only)
- Honest gates: bin tests **363**; tools **179**; coverage badge ~62%
- Dockerfile / Homebrew / VS Code / CONTRIBUTING / SUBMISSION\* — proprietary / UNLICENSED (not MIT)
- `deny.toml`: product = `LicenseRef-NextEleven-Proprietary`; MIT only as third-party dep allowance (not a product grant)

### Security
- No API keys in tracked tree; `.env` / `.envrc` gitignored
- Report vulns via SECURITY.md (private advisory)

## [0.1.2-beta] - 2026-05-25

### Added
- `[ambient]` config section and `AmbientProviders` (router fast for summaries, embed for vectors)
- Promotion docs: `docs/PROMOTION_REPORT.md`, `docs/RELEASE_NOTES_v0.1.2-beta.md`
- Refreshed `docs/COMPARISON.md` (Grok 4.x, MCP 2025, daemon, cost DB)

### Fixed
- TUI assistant label drift when router default differed from `[provider].model`
- Grok 4.1 Fast model slug (`grok-4.1-fast`)
- `harness export` and `harness delete` skip first-run setup wizard

### Changed
- `TODO.md` promotion tiers (Tier 0–3)
- `CONTRIBUTING.md` contribution pathways and community section
- README demo video placeholder

## [0.1.1-beta] - 2026-05-24

### Added
- `harness setup` — interactive provider and API key configuration
- `harness update` — prints platform-specific upgrade instructions
- TUI and web UI screenshots in README
- Windows prebuilt in release workflow
- Ollama fallback when no cloud API keys are configured
- MCP inbound request handling for `sampling/createMessage`
- Default MCP command allowlist in config

### Fixed
- Empty `ProviderRouter` panic when no providers configured
- Prebuilt download URL mismatch (`install.sh` ↔ GitHub Releases artifact names)
- XAI API key missing `.unwrap()` panic in `build_arc_provider`
- Stale default Claude model ID (`claude-sonnet-4-6`)
- AppleScript injection in calendar bridge paths
- Constant-time bearer token comparison
- Removed committed merge artifacts (`main.rs.orig`, `main.rs.rej`)

### Changed
- Install scripts warn when `~/.cargo/bin` and `~/.local/bin` both contain harness
- First-run wizard reloads config after saving keys
- `/api/setup/state` no longer exposes filesystem config path

## [0.1.0] - 2026-05-23

### Added
- Initial public release
- Multi-provider support (Anthropic, xAI, OpenAI, Ollama)
- Terminal TUI with ratatui
- Semantic memory + project memory
- Sub-agent swarm support
- Cost tracking and dashboard
- MCP client support
- Browser automation via Chrome DevTools Protocol
- Cross-machine encrypted sync
- GitHub PR review integration

### Notes
- First tagged release
- Prebuilt binaries available for macOS, Linux, and Windows
