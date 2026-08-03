# Collaborative multi-user sessions (W4.4)

Harness can share a live agent session over WebSocket when **collab** is enabled. Multiple clients join the same session id and receive agent stream events (text chunks, tool start/result, done) plus peer join/leave/typing.

## Config

```toml
[collab]
enabled = true
max_users = 10   # default 10; values < 1 clamp to 1
```

`max_users` is enforced on join: a new `user_id` is rejected with `collab session full (max N users)` when the session is at capacity. **Rejoining** the same `user_id` does not consume an extra seat.

## Endpoint

Requires `harness serve` (or the desktop/server path that mounts the HTTP app).

```
GET /ws/session/:id?token=<optional>&user_id=<optional>
```

| Query | Role |
|-------|------|
| `token` | Same bearer rules as other `/serve` routes when auth is configured |
| `user_id` | Stable client id; random `uXXXXXXXX` if omitted |

When `[collab].enabled` is false, the route returns **404**.

## Protocol (JSON text frames)

Server → client events (`type` tag, snake_case):

| type | Fields |
|------|--------|
| `user_joined` | `user_id` |
| `user_left` | `user_id` |
| `user_typing` | `user_id`, `partial` |
| `agent_text_chunk` | `content` |
| `agent_tool_start` | `name` |
| `agent_tool_result` | `name`, `preview` |
| `agent_done` | — |
| `session_info` | `session_id`, `user_count` |

Client → server (optional typing indicator):

```json
{"type":"typing","partial":"draft text…"}
```

Join capacity errors are sent as a plain text frame `error: collab session full (max N users)` then the socket closes.

## Smoke (local)

```bash
# Terminal A — start server with collab on (config above)
./target/debug/harness serve --addr 127.0.0.1:8787

# Terminal B/C — websocat / wscat (example)
websocat "ws://127.0.0.1:8787/ws/session/demo?user_id=alice"
websocat "ws://127.0.0.1:8787/ws/session/demo?user_id=bob"
```

Unit tests (no network): `cargo test --bin harness collab`

## Doctor

`harness doctor` prints collab enabled flag and `max_users`.

## Implementation map

| Piece | Path |
|-------|------|
| Config + registry + max_users | `src/collab.rs` |
| WS route | `src/server.rs` — `/ws/session/:id` |
| Agent → collab bridge | `agent_event_to_collab` in `server.rs` |

## Limits / non-goals

- No durable shared history beyond the live process registry (in-memory).
- Not a full CRDT editor — agent stream fan-out + typing only.
- Auth is the same serve token path; do not expose collab without network controls.
