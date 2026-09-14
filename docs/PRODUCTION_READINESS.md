# Production readiness — 2026-09-13

**Decision: hardened candidate; not yet a supported production release.** The local macOS CLI and HTTP paths pass the checks below, including one real provider round trip. Cross-platform CI and the complete release artifact matrix remain blocked by GitHub's account billing lock. No stable tag or production release was published as part of this work.

## Scope and starting state

Work began on clean `main` at `78e90ac7654779368b5a116fe9c30e2883396863`. The root Rust workspace, embedded browser UI, installers, release/CI workflows, Docker path, VS Code package, and separate Tauri crate were reviewed and repaired. This was release engineering and targeted runtime hardening, not an exhaustive independent security audit or acceptance of every optional integration.

The existing proprietary NextEleven LLC license and explicit user-owned provider/model route remain the product contract. Credentials and personal runtime state were not copied into the repository or release artifacts.

## Repairs

- **Browser startup:** restored initialization of `resumeLastSession`; its absence threw a ReferenceError and stopped the UI at “connecting…”. Tests now execute the actual embedded script with a fresh profile and a stored disabled-resume preference, and exercise authenticated streaming chat.
- **HTTP boundary:** reject foreign Host headers on loopback connections and cross-origin requests before token bootstrap, API handlers, or WebSocket upgrade. Responses use `no-store`, `nosniff`, and frame denial. Deployments using a loopback reverse proxy must account for this Host boundary; the supported local UI uses localhost or a loopback IP.
- **Cancellation:** browser SSE disconnect now cancels the agent/provider turn and persists completed message history, filling interrupted tool-call results so resumption remains valid. Shell timeout/cancellation kills the owned process and its Unix process group; regressions prove grandchildren cannot perform a delayed workspace write. Completed filesystem effects are not rolled back, and Windows descendant-tree termination remains a native acceptance concern.
- **Installers:** require exactly one matching SHA-256 entry, reject missing/mismatched checksums without replacing an existing installation, correct the Git Bash executable filename, preserve a requested version during source fallback, and stop generating vendor-selected configuration. `HARNESS_INSTALL_SOURCE=1` makes CI test the current checkout.
- **Homebrew:** verify all four Unix artifacts before changing the formula, update existing hashes as well as placeholders, and retain all checksum lines. No hashes were invented for unavailable releases.
- **Release workflow:** validate the existing tag, main ancestry, Cargo version, and successful latest CI and Coverage runs for the commit; check out the validated immutable commit on every native platform; require all five jobs and artifact files; stage a draft with checksums. Manual dispatch cannot silently label a main-branch build as an older release.
- **Validation:** pinned Rust, locked builds, deterministic installer tests, isolated CLI smoke, and synthetic real-binary provider/HTTP integration are wired into CI. Windows shell declarations and native runner labels were corrected. Coverage export reuses its measurement rather than rerunning the suite.
- **Sync:** filesystem tests use disposable paths and check an actual save/load round trip. Formatting and strict workspace lint were repaired without changing provider-routing behavior.
- **VS Code:** lockfile added, packaging dependencies updated, npm audit clean, and VSIX contents restricted to runtime files and license. Packaging recompiles TypeScript.
- **Desktop:** independent lockfile refreshed to remove vulnerable XML parsers; bounded HTTP readiness, standard CLI install-location lookup, reload after startup, and visible failure recovery added. Unused broad asset-protocol access disabled. The desktop remains a wrapper that requires the CLI to be installed separately.
- **Container path:** corrected the obsolete Rust image, invalid pre-FROM labels, missing build inputs, and runtime user. Excluded credential/runtime/build directories from the Docker context. Compose now uses separate container state and loopback-only Ollama exposure; setup instructions explicitly select the container endpoint.

## Direct validation

| Check | Result |
|---|---|
| Root `cargo test --locked --workspace --all-features` | **775 passed**, no failures; **454** are root binary tests |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | Passed |
| Root `cargo deny check` | Advisories, bans, licenses, and sources passed under existing policy |
| Measured coverage | **64.39% lines** (18112/28130), 66.19% regions, 70.98% functions; 60% gate passed |
| Optimized macOS arm64 CLI | `cargo build --locked --profile release-lto` passed |
| Explicit release-binary offline smoke | Passed; temporary home/project state, no caller-state mutation |
| Synthetic provider/HTTP integration | Passed: streaming, real read_file execution, tool-result continuation, exact model at HTTP boundary, bearer auth, session persistence, Markdown export |
| Live provider one-shot | Passed on the existing `openai-compatible` route with `deepseek-ai/deepseek-v4-flash-0731`; expected `HARNESS_LIVE_OK` returned, exit 0, no tools requested |
| Real browser | Observed initial startup failure, then verified ready state and `HARNESS_SMOKE_OK` chat after repair using a disposable synthetic backend |
| Embedded UI DOM regressions | 2 passed; npm audit reports zero vulnerabilities |
| Cancellation regressions | Passed: provider future dropped on disconnect, session persisted with complete tool protocol, Unix descendants cannot make delayed writes after timeout or cancellation |
| Installer/Homebrew contracts | 4 passed, including invalid/missing/duplicate checksums, Git Bash filename, and all-or-nothing formula update |
| Workflow and shell validation | `actionlint`, `shellcheck` for changed shell scripts, YAML parsing passed |
| VS Code extension | TypeScript compile and VSIX packaging passed; npm audit reports zero vulnerabilities |
| Tauri dependency update | Final locked check and local optimized macOS .app bundle passed; Developer-ID signing/notarization and clean-machine GUI acceptance remain open |
| Desktop advisories | Vulnerability gate passes with six explicit maintenance-only exceptions in its separate deny.toml |

Coverage is a workspace measurement, not evidence that optional APIs, every provider, every OS, or GUI packaging have been accepted. The live provider smoke is one bounded request; it is not a throughput, reliability, or billing-accuracy benchmark.

## Desktop maintenance exceptions

The updated Tauri graph still requires six crates with upstream unmaintained advisories and no patched versions. These are explicitly accepted maintenance risks for the candidate, not suppressed vulnerability findings:

- `RUSTSEC-2024-0370`: proc-macro-error through GTK/glib-macros.
- `RUSTSEC-2025-0081`, `RUSTSEC-2025-0075`, `RUSTSEC-2025-0080`, `RUSTSEC-2025-0100`, `RUSTSEC-2025-0098`: the unic dependency family through Tauri's URL-pattern parser.

[Desktop advisory policy](../apps/desktop/src-tauri/deny.toml) lists each ID and the requirement to recheck on framework upgrades. Newly reported vulnerabilities still fail the gate. The root workspace retains its pre-existing maintenance exceptions separately.

## Remaining gates and restart order

1. **Restore GitHub Actions execution.** The latest starting-commit run, [34740550198](https://github.com/seanebones-lang/harness/actions/runs/34740550198), reports: “The job was not started because your account is locked due to a billing issue.” Restore account billing, then run CI for the final candidate commit. A billing-rejected job is not a test failure or a pass.
2. **Run the full native matrix and install rehearsals.** macOS Intel, Linux arm64/x86_64, and Windows must execute the final candidate tests and installers. This Mac cannot establish their behavior. Docker CLI exists locally, but its daemon socket is unavailable, so the repaired container path remains unexecuted.
3. **Complete interactive acceptance.** Real-provider TUI approval/cancellation, browser reconnect/cancellation, native editor integration, and cold-start desktop GUI behavior on clean machines remain distinct checks. One-shot live success and a synthetic browser chat do not complete that matrix.
4. **Complete desktop distribution requirements.** Validate final GUI packages on each supported platform, Developer-ID signing/notarization on macOS, and required OS-specific installer behavior. The wrapper requires a separately installed/configured CLI.
5. **Cut a new immutable version after acceptance.** Align all versions, tag the green candidate, build all five verified artifacts, complete checksums, publish the staged draft, refresh Homebrew hashes, and validate public installer downloads. Do not move or overwrite the existing v1.3.0 tag.

## Repeat local checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo deny check
cargo build --locked --profile release-lto
HARNESS_BIN="$PWD/target/release-lto/harness" bash scripts/smoke_rel01.sh
python3 scripts/smoke_agent.py target/release-lto/harness
python3 scripts/test_install.py
npm ci --prefix static
npm test --prefix static
npm ci --prefix extensions/vscode
npm run package --prefix extensions/vscode
cargo check --locked --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo deny --manifest-path apps/desktop/src-tauri/Cargo.toml --config apps/desktop/src-tauri/deny.toml check advisories
actionlint
```

[Release process](RELEASE_PROCESS.md) · [container instructions](CONTAINERS.md) · [coverage](../COVERAGE.md) · [release log](RELEASE_STATUS.md)
