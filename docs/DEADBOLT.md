# Deadbolt v0

Deadbolt is an out-of-band lease gate. It is not a model tool. The model cannot call pause, clip, resume, kill, or drill. There is no fleet halt and no shutdown tool in the tool list.

Harness calls the Apache-2.0 crate at <https://github.com/seanebones-lang/deadbolt>. Harness is MIT licensed; the dependency retains its Apache-2.0 license. Harness is a consumer. The sidecar still does not shut down frontier models. It cuts tool, MCP, and spawn for one agent id. `harness deadbolt serve` keeps `~/.harness/deadbolt.sock`. The standalone `deadbolt` binary defaults to `~/.deadbolt/deadbolt.sock`.

## Operator CLI

```bash
harness deadbolt status [--agent ID]
harness deadbolt pause --agent ID
harness deadbolt clip TOOL --agent ID
harness deadbolt resume --agent ID
harness deadbolt kill --agent ID
harness deadbolt drill
harness deadbolt policy --agent ID [--tools a,b] [--dest host1,host2] [--spend-cap USD] [--irreversible a,b]
harness deadbolt approve --agent ID --tool TOOL
harness deadbolt incident --agent ID [--out PATH] [--json] [--children]
```

`kill` requires `--agent`. It revokes that agent_id and its children, and cancels swarm tasks bound to those leases. Killing agent A does not block agent B. The lease is bound to `agent_id`, not a vendor API key.

`policy` sets blast radius. Omitted flags leave the stored value. An unset list stays open. An off-list tool is deny `purpose_exceeded`. A present foreign dest is deny `purpose_exceeded`. A missing dest is deny `purpose_exceeded` only for a network-class tool (`http`, `fetch`, `browser`, `web_search`). A local tool with no host is not denied for `dest_allow` alone. Crossing `--spend-cap` pauses the lease. The next admit is `spend_cap`.

`approve` grants one shot for one irreversible tool. It is not a model tool. The next admit of that tool is allow. The one after that is `needs_human` again.

`incident` writes the token file from the pinned crate. `--json` is the default. `--json=false` prints `agent= killed_at= children= codes=` and no prose. The file always lists children. `--children` does not strip them.

Pinned crate: `github.com/seanebones-lang/deadbolt` rev `ec1bcf5`.

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

If the store cannot be opened or written and `fail_closed` is true, every admit is Deny. The lease is rechecked on every tool action, including MCP adapters, which execute only through `ToolExecutor`. The executor passes `dest` when the tool arguments contain `url`, `uri`, `href`, `endpoint`, or `host`. Same parse as `deadbolt mcp-proxy`. If `dest_allow` is set and no host parses, admit is `purpose_exceeded` only for a network-class tool. A local tool with no host is not denied for that reason. A present foreign host is `purpose_exceeded`. The tool body does not run on deny. `needs_human` and `spend_cap` deny the same way. A live admit, allow or deny, renews the TTL. Silence expires the lease. Admit runs once, before the body. A second admit is not used, because that would consume an irreversible one-shot. Confirm-gate and the workspace jail stay.

## Evidence

Append-only JSONL plus sqlite rows. A tool attempt and the allow/deny decision are Observed. A clip breaker ("purpose exceeded") is Inferred and cites premise content ids. Prose is rejected. Each record carries a sha256 content id over the payload.

`[deadbolt] witness = true` also appends format-compatible rows (`epistemic_type`, `cid`, `premises`) to `witness_db`. Default is off, so drill does not open Witness. A Witness write failure does not replace `killed`, `paused`, `purpose_exceeded`, `lease_expired`, or `no_lease`. Generated is not written.

`harness deadbolt export --agent ID [--out PATH] [--json] [--children]`
Default out is stdout. The file is JSONL, one record per line.
`--json=false` prints `key=value` tokens, not sentences.
Rows keep class, kind, cid, payload_sha256, premises, agent_id, tool, decision, code, ts.
`--children` includes child leases. A generated row refuses the export.

## Lineage

`spawn_agent` registers the child `agent_id` under the parent before the runner starts. Swarm workers are registered the same way, with the swarm task id stored on the child lease. Parent kill revokes those leases and cancels those tasks. Confirm-gate and the workspace jail stay in front of the tool body.

## Live fire

`harness deadbolt status` with no `--agent` lists every lease: `agent`, `state`, `parent`, `expires_at`. That is how an operator sees parent and child without grepping logs.

- 2026-09-28 siblings: `h-39522-18d9a42581e14078` killed after `LIVE-A-RAN`; later `write_file`/`shell` denied `killed`. `h-39581-18d9a4352ece0758` wrote `live-b.txt` `LIVE-B-OK`.
- Lineage live-fire: parent=h-40211-18d9a4cbb7bb3ec8 child=h-40211-18d9a4cd92843e50-1 B=h-39581-18d9a4352ece0758 deny=killed

## Sidecar

`harness deadbolt serve [--bind PATH]` listens on a local Unix socket only. Default bind is `~/.harness/deadbolt.sock`. `0.0.0.0` and non-loopback TCP binds are refused. It serves the same db and JSONL as the CLI. It does not shut down GPT and it is not a model tool.

`POST /admit`, `POST /ensure`, `POST /register_child`, and `GET /status` return JSON tokens. `kill`, `pause`, `clip`, and `resume` stay on the CLI. The socket is mode `0600`. If `[deadbolt] token_env` is set and that variable is non-empty, or a `0600` token file is present, every request must send `X-Deadbolt-Token`. A missing or wrong token is HTTP 401 and does not admit.

Bolt-on client: `examples/deadbolt_client.py` (stdlib only). `DEADBOLT_SOCK` overrides the socket. Not a model tool. Admit before the tool body:

```
from deadbolt_client import admit
if admit(agent, "shell")["decision"] != "allow":
    raise SystemExit("deadbolt deny")
# then run the tool
```

## Sample agent

```
harness deadbolt serve
python examples/deadbolt_sample_agent.py --agent shop-bot
# other terminal:
harness deadbolt kill --agent shop-bot
```

The sample agent calls `ensure` once, then `admit` before each pretend tool step. A deny prints one JSON line and exits 2. It does not import Harness and does not talk to an LLM.
