# Personal-use assessment and acceptance

Updated October 9, 2026. This document governs the current work; historical public-release records are retained separately.

## Direction and decisions

Harness is Sean McDonnell's personal coding tool under the existing MIT license. Keep the Rust provider abstraction, exact user-selected routes, repository tool executor, SQLite sessions, terminal UI, local web interface, MCP/LSP, and bounded workers. Repair their runtime contracts before expanding the feature surface.

Competition and promotion documents have moved to `docs/history/`. Their earlier policies and claims do not govern current work. Signed distribution, marketplace publication, and commercial launch are outside personal-use acceptance. Optional integrations remain available and must be verified when used.

The local uncommitted `harness-agents` and macro experiment was preserved in a local recovery archive and the original Git stash. It implements a second, incomplete runner with placeholder MCP/agent tools, no concrete model adapter, and simulated streaming. It was excluded from the active build rather than duplicating the existing runtime.

A stale-file-handle failure affected the Desktop Git object store. The current upstream was independently cloned and its source and Git metadata restored. Original metadata and local edits remain recoverable. Current tests run from the clean checkout outside Desktop to avoid repeating the file-provider failure. The Desktop working tree is synchronized, with its active Git metadata redirected outside Desktop after another stalled refresh.

## Repairs

- First-party Cargo, Docker, Homebrew, desktop, web, VS Code, and contribution metadata now agree on MIT; third-party licenses remain separate.
- Text previews and shell output caps preserve UTF-8 boundaries. Session exports retain complete tool results.
- Stream adapters buffer raw transport bytes until full lines arrive, preserve combined text/tool/usage/terminal events, and report incomplete responses as errors.
- Official OpenAI reasoning-model requests use `max_completion_tokens` and omit unsupported sampling parameters. Other compatible endpoints retain their request format. Embedding requests honor the selected model and preserve HTTP error status.
- A fallback uses its own configured model rather than sending the primary model to a different backend.
- Token and tool-loop limits return failure. Failed or cancelled one-shot runs persist resumable history; terminal quit cancels the active turn and saves its ledger. Partial responses are recorded as they stream.
- Compaction preserves history on failure, cancellation, empty summaries, or incomplete responses, and keeps tool-call/result groups in complete user turns. Ambient consolidation also preserves original memories on errors, EOF, or token limits. GPT-OSS compaction uses its documented 131,072-token window rather than the broad GPT estimate.
- Terminal failures stay visibly stopped rather than being relabeled complete. Session naming requires a completed response.
- `harness connect` handles byte-safe SSE and returns failure for server errors or unexpected EOF.
- `make verify` provides a single local quality/acceptance command. A POSIX PTY test exercises the actual terminal UI.

## Reference sources

OpenAI repositories inspected: [Codex](https://github.com/openai/codex), [Agents SDK runner](https://github.com/openai/openai-agents-python/blob/main/src/agents/run.py), and [Python SDK request schema](https://github.com/openai/openai-python/blob/main/src/openai/types/chat/completion_create_params.py). [The official Chat Completions reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create) establishes current token-budget parameters. The [GPT-OSS model reference](https://developers.openai.com/api/docs/models/gpt-oss-20b) documents its context limit; the [GPT-OSS repository](https://github.com/openai/gpt-oss) and [NVIDIA endpoint](https://build.nvidia.com/openai/gpt-oss-20b) were also inspected. These are implementation references, not a promise of SDK or API parity; no wholesale upstream source import was made.

## Verified baseline

Remote source baseline: `8915ea79f3bd681a64e1a119da85b2c3df1608d1`. Its [13-job CI run](https://github.com/seanebones-lang/harness/actions/runs/38000152587) and coverage run passed. Local locked workspace baseline passed 786 tests including doctests. This evidence belongs to the starting commit.

## Candidate acceptance — October 9, 2026

- `make verify`: formatting, strict workspace Clippy, 803 workspace tests including doctests, CLI smoke, authenticated HTTP/SSE, repository `read_file` execution and tool-result continuation, exact model routing, complete Markdown export, terminal UI, cancellation recovery, and four installer contract tests passed.
- Default-feature-disabled compilation and strict Clippy passed. CI now checks this configuration too.
- `cargo deny check`: advisories, bans, licenses, and sources passed. Existing informational warnings about unmatched dependency exceptions remain.
- Embedded web UI: both DOM regression tests passed; npm audit reported no vulnerabilities. VS Code extension compilation and npm audit passed.
- Optimized `release-lto` compilation passed on the owner's Apple Silicon Mac.
- The owner's saved NVIDIA model returned HTTP 410 and was absent from the current authenticated model list. Its listed DeepSeek successor passed bounded text requests, but tool requests timed out in both the streaming Harness check and an independent minimal endpoint probe. A listed alternative, `openai/gpt-oss-20b`, passed the full live file-read/tool-result loop through Harness: two successful requests, actual `read_file` execution, expected file contents, and a saved session. The explicit NVIDIA route now selects this verified model.
- An incomplete model-less Ollama fallback and retired optional role overrides were removed. Ambient/vector memory is disabled until its embedding backend is configured; stored memories and SQLite sessions are retained. The worker model is explicitly aligned with the available primary model. The original private configuration was backed up; credentials were neither printed nor copied into the repository.
- Both installed executables were refreshed. Optimized-binary CLI, HTTP, tool, and terminal/cancellation smoke passed, followed by terminal startup and clean quit using the owner's actual configuration in the Desktop workspace.
- Final code commit `af610901328f713e69af6b7af5507749a04caca5` passed all [13 CI jobs](https://github.com/seanebones-lang/harness/actions/runs/38003249253) and [coverage](https://github.com/seanebones-lang/harness/actions/runs/38003249262). This includes Ubuntu, macOS, and Windows builds/tests/installers, container runtime acceptance, strict linting, the default-feature-disabled configuration, desktop/extension compilation, web regressions, and supply-chain checks.

## Build workflow acceptance — October 9, 2026

Harness now saves an editable outcome, observable acceptance criteria, and verification commands in `.harness/build.toml`. `harness build` continues that outcome; `--resume` also restores a saved conversation. The CLI, terminal, and HTTP agent share the brief and prior progress context. Replacing an outcome archives its brief and progress together. Offline preparation makes no provider calls. New project initialization supplies a local-use next step and ignores runtime state, credentials, dependencies, and build artifacts.

The brief instructs the model to verify a useful slice against the existing stack, preserve retrieval sources, handle empty retrieval honestly, and account for automation retry/recovery behavior where relevant. These are agent instructions, not an automatic acceptance certifier. Test execution now honors failed command exit status, accumulates Rust workspace summaries, and reports unparsed counts honestly. Unix test cancellation cleans up the owned process group.

The executable build integration check uses a synthetic provider with real file writes and two executed Python retrieval tests. It verifies credential-free preparation, acceptance/project instructions reaching the provider, and fresh-session continuation from saved progress. It does not benchmark an autonomous full RAG application.

The extended candidate passed `make verify`: 810 workspace tests including doctests, strict formatting/Clippy, no-default-feature Clippy, installer contracts, and CLI/HTTP/build/terminal smoke checks.

The first bounded live build on the saved NVIDIA GPT-OSS model returned eight HTTP 200 responses but did not finish the small retrieval component within that request cap. A malformed tool name containing a channel marker was blocked. Unknown tools now return exact registered names alongside any admission denial, without aliasing the malformed name or executing its body. A regression test confirms that boundary. Live build capability must be assessed separately from the deterministic workflow checks.

## Limits of this acceptance

This establishes a personal Mac CLI/terminal and loopback HTTP workflow. Other providers have local parser/request tests, not paid live acceptance. Desktop/editor compilation does not establish interactive operation of every optional client, MCP server, voice backend, computer control, or remote swarm. Verify each optional integration against the actual service when it is used. No signed distribution, marketplace release, or commercial-readiness claim is made.

Personal-use acceptance is complete for the workflow above. No known blocking defects remain in that accepted workflow. Both source checkouts are synchronized and the optimized executable is installed. Local logs and the source/binary receipt are retained with the recovery archive. Subsequent documentation-only acceptance updates do not alter the tested executable or source contracts.

Remote CI results belong to their exact code commit and remain available in [GitHub Actions](https://github.com/seanebones-lang/harness/actions).
