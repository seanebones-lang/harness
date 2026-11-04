# DeadBolt in Harness

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
harness deadbolt policy --agent ID [--tools a,b] [--dest host1,host2] [--offline-tools a,b] [--spend-cap USD] [--irreversible a,b]
harness deadbolt approve --agent ID --tool TOOL
harness deadbolt incident --agent ID [--out PATH] [--json] [--children]
```

`kill` requires `--agent`. It revokes that agent_id and its children, and cancels swarm tasks bound to those leases. Killing agent A does not block agent B. The lease is bound to `agent_id`, not a vendor API key.

`policy` sets blast radius. Omitted flags leave the stored value. An unset list stays open. An off-list tool or present foreign destination denies `purpose_exceeded`. With a destination allow-list set, every tool needs a declared destination unless the operator explicitly names it in `--offline-tools`. Offline exceptions default to none; an exception never overrides a foreign destination. Classify only tools you trust to have no network effects. Do not classify a general-purpose shell as offline merely to make a denied call run. These declarations do not enforce OS egress restrictions. Crossing `--spend-cap` pauses the lease; later admissions deny `spend_cap`.

`approve` grants one shot for one irreversible tool. It is not a model tool. The next admit of that tool is allow. The one after that is `needs_human` again.

`incident` writes the token file from the pinned crate. `--json` is the default. `--json=false` prints `agent= killed_at= children= codes=` and no prose. The file always lists children. `--children` does not strip them.

Pinned crate: `github.com/seanebones-lang/deadbolt` at full revision
`e87519f700be03c27a564ed274907649224d256f` (released `2.0.1` source). Harness pins this revision; it does not follow a moving branch. Updating Harness source does not modify already downloaded Harness binaries.
The standalone dependency is free under Apache-2.0; Harness remains MIT.

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

If the store cannot be opened or written and `fail_closed` is true, every admit is Deny. The lease is rechecked on every tool action, including MCP adapters, which execute only through `ToolExecutor`. The executor passes the first host parsed from recognized `url`, `uri`, `href`, `endpoint`, or `host` argument fields. This legacy adapter is not the upstream raw MCP proxy's complete destination collector: it does not establish all actual network destinations. Keep OS egress controls separate. With a destination policy set, an absent host requires an explicit offline exception; a foreign host denies. The tool body does not run on deny. `needs_human` and `spend_cap` deny the same way.

Harness explicitly preserves its existing **activity-renewed** root setup through `ensure_activity_agent`. A live activity admission, allow or deny, can renew the idle TTL; silence expires it. This is not independent supervisor liveness. Existing identities retain supervision mode, expiry, hard deadline and terminal state; startup never revives a killed or expired run or slides a hard deadline. Harness does not add deadline issuance or supervisor/grant management UI in this integration. Operators needing independent liveness must provision and renew supervision separately through the standalone engine, using the same private store.

For registered tools, admission runs once immediately before the effect, after
any interactive confirmation. If policy, kill, pause or expiry changes during
review, that final admission sees it. Declining review does not consume a
DeadBolt approval or record caller-reported spend. The reviewed-content write
path uses the same final admission boundary. Confirm-gate and the workspace jail stay. Caller-reported `amount`/`usd`/`cost` is recorded only after an allowed admission; denied calls do not change spend or add spend evidence. This is self-reported accounting, not a verified provider invoice or a preflight price reservation. Reaching a cap affects subsequent admissions.

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

`POST /admit`, `POST /ensure`, `POST /register_child`, and `GET /status` return JSON tokens. `kill`, `pause`, `clip`, and `resume` stay on the CLI. The socket is mode `0600`. Version 2 requires an operator token for sidecar startup by default. Configure a nonempty `[deadbolt] token_env` or private mode-0600 `token_file`; operator requests must send `X-Deadbolt-Token`. A missing or wrong token does not admit. Do not enable the upstream legacy unauthenticated compatibility option to avoid setup. Keep operator credentials out of agent-held tools; use the standalone scoped-admission client where that trust split is needed.

Bolt-on client: `examples/deadbolt_client.py` (stdlib only). `DEADBOLT_SOCK` overrides the socket. Not a model tool. Admit before the tool body:

```
from deadbolt_client import admit
if admit(agent, "shell")["decision"] != "allow":
    raise SystemExit("deadbolt deny")
# then run the tool
```

## Sample agent

```
# Trusted operator fixture only: configure token_env = "DEADBOLT_TOKEN" first.
# Supply its private value through your existing secure shell workflow.
harness deadbolt serve
python examples/deadbolt_sample_agent.py --agent shop-bot
# other terminal:
harness deadbolt kill --agent shop-bot
```

The sample is a trusted operator fixture, not an untrusted agent credential example: it needs the same operator authentication in its environment. It calls `ensure` once, then `admit` before each pretend tool step. New HTTP roots use the engine's supervisor default and expire after the idle TTL without separate renewal. A deny prints one JSON line and exits 2. It does not import Harness and does not talk to an LLM.

## Candidate integration acceptance

The November 3 source migration pins released engine 2.0.1. Its focused local
checks passed 203 tests: two wrapper contracts and 201 tool/executor tests. New
checks cover denied calls leaving spend and spend evidence unchanged, one
allowed call accounting once, destination policy requiring an explicit offline
exception, a foreign destination remaining denied despite that exception, and
activity compatibility preserving existing supervision/deadlines/terminal state.
The prior local-tool/no-destination test expectation was changed to the new
secure default, rather than adding implicit exceptions. Root CLI `cargo check`
also passed. These are local source checks; published Harness binaries and the
full platform/release matrix require their own qualification.

The existing wrapper/tool suites passed with both the previous dependency and
the pinned candidate. Seven additional executor regressions cover policy changes
and lease expiry during review, operator kill during review, declined review
preserving a one-shot, clipped reviewed-content writes, successful reviewed-content
writes consuming once, and exact-only policy refusing legacy dispatch. The first
four review-timing regressions failed before the dispatcher correction.

Harness currently uses the legacy admission path. This dependency update does
**not** expose exact-action approval or credential issuance in the Harness CLI,
or make its human review a payload-bound DeadBolt grant. When a tool is configured
exact-only in the underlying store, this legacy executor returns `needs_human`
and does not run the body. A future exact-action adapter must bind the actual
validated effect arguments and consume through the exact-action API; do not
replace this refusal with a legacy fallback.

Run `cargo test --locked -p harness-deadbolt -p harness-tools` for the focused
acceptance. Tests use disposable state and synthetic tools; no model API requests
or real email/text sends occur. The full workspace/CI remains a separate gate.
Post-write autoformat/test hooks and arbitrary code not routed through this
executor are not proven contained by these tests. Kill gates later admission;
any cancellation behavior remains owned by Harness.

The pinned source is the integrated DeadBolt release commit. Check the
[versioned engine release](https://github.com/seanebones-lang/deadbolt/releases/tag/v2.0.1)
for download availability and exact acceptance evidence. This Harness adapter
uses legacy admission; it does not issue exact-action grants automatically.
