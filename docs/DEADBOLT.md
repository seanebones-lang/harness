# Deadbolt v0

Deadbolt is an out-of-band lease gate. It is not a model tool. The model cannot call pause, clip, resume, kill, or drill. There is no fleet halt and no shutdown tool in the tool list.

Harness calls Deadbolt. Deadbolt writes evidence through a thin Witness-shaped sink (Observed / Inferred / Generated, premises on inferences, sha256 content id). This crate does not vendor Witness or EvidenceLens and does not depend on the TUI or provider types.

## Operator CLI

```bash
harness deadbolt status [--agent ID]
harness deadbolt pause --agent ID
harness deadbolt clip TOOL --agent ID
harness deadbolt resume --agent ID
harness deadbolt kill --agent ID
harness deadbolt drill
```

`kill` requires `--agent`. It revokes that agent_id and its children, and cancels swarm tasks bound to those leases. Killing agent A does not block agent B. The lease is bound to `agent_id`, not a vendor API key.

`drill` is an in-process self-check. It does not need API keys and does not touch `~/.harness`.

## Config

```toml
[deadbolt]
enabled = true
fail_closed = true
lease_ttl_secs = 60
db_path = "~/.harness/deadbolt.db"
events_path = "~/.harness/deadbolt-events.jsonl"
```

If the store cannot be opened or written and `fail_closed` is true, every admit is Deny. The lease is rechecked on every tool action, including MCP adapters, which execute only through `ToolExecutor`.

## Evidence

Append-only JSONL plus sqlite rows. A tool attempt and the allow/deny decision are Observed. A clip breaker ("purpose exceeded") is Inferred and cites premise content ids. Prose is rejected. Each record carries a sha256 content id over the payload.

## Lineage

`spawn_agent` registers the child `agent_id` under the parent before the runner starts. Swarm workers are registered the same way, with the swarm task id stored on the child lease. Parent kill revokes those leases and cancels those tasks. Confirm-gate and the workspace jail stay in front of the tool body.
