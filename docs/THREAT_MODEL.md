# Threat model — harness (May 2026)

This document describes what harness is **designed** to trust, what it **must not** expose, and how to deploy it safely.

## Trust boundaries

| Component | Trust assumption | Untrusted input |
|-----------|------------------|-----------------|
| Local user | Full trust | — |
| LLM provider | Semi-trusted (prompt injection) | Model output, tool args |
| MCP servers | Configured by user | Tool schemas, spawned processes |
| Sync git remote | User-controlled private repo | Encrypted tarball contents |
| Network clients | **Untrusted** unless authenticated | HTTP/daemon requests |

Harness is a **local coding agent**: it runs shell commands, edits files, and loads MCP tools on behalf of the operator. Treat it like `sudo` for your workspace.

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

- **Workspace sandbox:** Filesystem, search, git, and apply_patch tools resolve paths under the project root (`WorkspaceRoot`, strict by default).
- **Shell / MCP:** Not sandboxed to the repo. MCP roots exclude `$HOME`; sampling requires approval when configured.
- **Plan mode:** Destructive tools (`write_file`, `patch_file`, `shell`, `apply_patch`, MCP tools) pause for confirmation when `--plan` or `[approval].mode = "plan"`.
- **Computer use:** Screen/keyboard control when `[computer_use] enabled = true` — equivalent to remote desktop access.

## Sync (`harness sync`)

- State encrypted with age; passphrase in Keychain or `~/.harness/.sync-key`.
- **Tar-slip:** Pull validates paths stay under `~/.harness/` (regression-tested).
- **Risk:** Compromised sync repo + weak passphrase → decrypted session/memory exfiltration.

## Recommendations

1. Keep HTTP/daemon on loopback in development; use VPN + auth for remote access.
2. Run `harness doctor` after install; verify token files are `0600`.
3. Use `--plan` for untrusted prompts or new MCP servers.
4. Do not enable `[computer_use]` on shared machines.
5. Record manual smoke results in `docs/RELEASE_STATUS.md` before stable release.

See also: [`docs/PEER_REVIEW_AUDIT.md`](PEER_REVIEW_AUDIT.md), [`docs/PUBLIC_RELEASE.md`](PUBLIC_RELEASE.md).
