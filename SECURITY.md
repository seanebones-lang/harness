# Security Policy

## Supported Versions

| Version     | Supported |
| ----------- | --------- |
| 0.1.1-beta  | Yes       |
| main branch | Yes       |

## Reporting a Vulnerability

1. **Do not** open a public GitHub issue for security-sensitive reports.
2. Use [GitHub private vulnerability reporting](https://github.com/seanebones-lang/harness/security/advisories/new) or email **seanebones-lang@users.noreply.github.com** (maintainer contact via GitHub profile).
3. Include steps to reproduce, affected versions, and potential impact.

We aim to respond within 48 hours.

## Threat Model

Harness is a local-first coding agent. Primary trust boundaries:

- Shell and file tools (user approval modes)
- MCP server subprocess spawning (command allowlist)
- HTTP server and daemon IPC (loopback bearer tokens)
- LLM provider network calls

Full detail: [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md).
