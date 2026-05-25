# Security Policy

## Supported Versions

| Version     | Supported |
| ----------- | --------- |
| 0.1.2-beta  | Yes       |
| 0.1.1-beta  | Yes       |
| main branch | Yes       |

## Reporting a Vulnerability

1. **Do not** open a public GitHub issue for security-sensitive reports.
2. Use [GitHub private vulnerability reporting](https://github.com/seanebones-lang/harness/security/advisories/new) or email **seanebones-lang@users.noreply.github.com** (maintainer contact via GitHub profile).
3. Include steps to reproduce, affected versions, and potential impact.

We acknowledge within **24 hours** and respond with a full assessment within **72 hours**. We follow coordinated disclosure: we work with reporters to understand, fix, and publish a joint advisory before public disclosure. We target a fix within 14 days of confirmation.

## Known Advisories

None at this time. All P0 security findings from the May 2026 peer review audit have been remediated — see [`docs/PEER_REVIEW_AUDIT.md`](docs/PEER_REVIEW_AUDIT.md).

## Security Design Highlights

- **Workspace sandbox**: file and git tools are jailed to the workspace root — path traversal is rejected
- **Confirm gate**: destructive tool calls require explicit user approval before execution
- **HTTP bearer auth**: `/api/*` routes require `Authorization: Bearer <token>` — no unauthenticated access
- **Daemon IPC**: Unix socket (macOS/Linux) or loopback TCP (Windows) — not exposed to the network
- **Supply-chain**: `cargo deny` checks licenses and bans; `cargo audit` scans for CVEs in CI

## Threat Model

Harness is a local-first coding agent. Primary trust boundaries:

- Shell and file tools (user approval modes)
- MCP server subprocess spawning (command allowlist)
- HTTP server and daemon IPC (loopback bearer tokens)
- LLM provider network calls

Full detail: [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md).
