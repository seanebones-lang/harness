# Threat model — harness v2 (2026-08-03)

This document describes what harness is **designed** to trust, what it **must not** expose, and how to deploy it safely after Wave 5 tools/providers and Wave 7 swarm gates.

## Trust boundaries

| Component | Trust assumption | Untrusted input |
|-----------|------------------|-----------------|
| Local user | Full trust | — |
| LLM provider | Semi-trusted (prompt injection) | Model output, tool args |
| MCP servers | Configured by user | Tool schemas, spawned processes |
| Sync git remote | User-controlled private repo | Encrypted tarball contents |
| Network clients | **Untrusted** unless authenticated | HTTP/daemon requests |
| Swarm workers | Same as parent user; **tool allowlist** when configured | Worker prompts / model output |
| Optional tools (DB/Docker/Notebook) | Off by default | SQL, docker CLI, notebook JSON |

NextEleven Harness is a **local coding agent**: it runs shell commands, edits files, and loads MCP tools on behalf of the operator. Treat it like `sudo` for your workspace.

## HTTP server (`harness serve`)

- **Default bind:** loopback (`127.0.0.1:8787`).
- **Auth:** Bearer token in `~/.harness/server.token` (mode `0600`). Required for `/api/chat`, sessions, projects, and setup persist.
- **Bootstrap:** `/api/health` on loopback returns the token for the bundled Web UI.
- **Risk:** Binding to `0.0.0.0` without a firewall exposes full agent + project test RCE to the network. **Do not** expose publicly without TLS, auth, and rate limits.
- **CSRF:** Browser UI uses same-origin fetch with the bearer token; cross-origin sites cannot read the token from loopback.

## Daemon IPC (`harness daemon`)

- **Transport:** Unix socket (`~/.harness/daemon.sock`) on macOS/Linux; loopback TCP + `daemon.port` on Windows.
- **Auth:** Token in `~/.harness/daemon.token`; verified on every request.
- **Risk:** Any local process that reads the token can drive the agent. File permissions and full-disk encryption are operator responsibilities.

## Tool execution

- **Workspace sandbox:** Filesystem, search, git, apply_patch, notebook, and database path tools resolve under the project root (`WorkspaceRoot`, strict by default).
- **Shell / MCP:** Not sandboxed to the repo. MCP roots exclude `$HOME`; sampling requires approval when configured.
- **Plan mode:** Destructive tools pause for confirmation when `--plan` or `[approval].mode = "plan"`.
- **Optional tools (default off):**
  - `database` — SQLite only; `readonly=true` rejects non-SELECT/WITH/PRAGMA/EXPLAIN.
  - `docker` — allowlisted read-heavy CLI; no run/build/rm; `compose_up` only if `allow_mutating`.
  - `notebook` — structure-only `.ipynb` JSON; writes checkpointed.
- **Computer use:** Screen/keyboard control when `[computer_use] enabled = true` — equivalent to remote desktop access. See `docs/COMPUTER_USE.md`.
- **Swarm workers:** `[swarm] worker_tool_allowlist` + `worker_max_wall_secs` reduce blast radius for parallel agents. Empty allowlist → safe defaults (`read_file`, `list_dir`, `search_code`, `test_runner`). Remote registry URL is stubbed (errors until W7.1 HTTP lands).

## Providers

- Keys via env / config only; never commit secrets.
- Gemini uses OpenAI-compatible Google endpoint; Bedrock uses AWS SigV4 Converse — credentials are standard AWS env vars.
- Smart router auto-detects keys; missing keys must fail closed without sending prompts.

## Sync (`harness sync`)

- State encrypted with age; passphrase in Keychain or `~/.harness/.sync-key`.
- **Tar-slip:** Pull validates paths stay under `~/.harness/` (regression-tested).
- **Risk:** Compromised sync repo + weak passphrase → decrypted session/memory exfiltration.

## Recommendations

1. Keep HTTP/daemon on loopback in development; use VPN + auth for remote access.
2. Run `harness doctor` after install; verify token files are `0600`.
3. Use `--plan` for untrusted prompts or new MCP servers.
4. Leave `[tools.database|notebook|docker]` and `[computer_use]` disabled until needed.
5. Prefer swarm worker allowlists for multi-agent batches.
6. Record manual smoke results in `docs/RELEASE_STATUS.md` before stable release.

## External audit checklist (pre-0.2.0)

- [ ] Dependency audit: `cargo deny check` + `cargo audit` green on release tag
- [ ] Secret scan of git history (no keys/tokens)
- [ ] HTTP: no `/api/*` without bearer; non-loopback bind docs warn
- [ ] Daemon: token required; socket permissions
- [ ] Workspace jail: path traversal tests for file/git/notebook/database
- [ ] Shell denylist + confirm gate still enforced for MCP-wrapped shell
- [ ] Swarm: allowlist + wall timeout behavior verified; remote registry disabled by default
- [ ] Optional tools default **off** in shipped `config/default.toml`
- [ ] Computer-use model gate + OS tool requirements documented
- [ ] Threat model reviewed against actual `docs/CTO_BACKLOG.md` feature set
- [ ] Coordinated disclosure contact current in `SECURITY.md`

See also: [`docs/PEER_REVIEW_AUDIT.md`](PEER_REVIEW_AUDIT.md), [`docs/PUBLIC_RELEASE.md`](PUBLIC_RELEASE.md), [`SECURITY.md`](../SECURITY.md).
