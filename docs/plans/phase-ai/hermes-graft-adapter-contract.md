# Hermes Graft Adapter Contract

## Scope

`atm_graft_hermes_bridge.HermesGraftBridge` is the ATM-core reference adapter
for one Hermes profile. It creates one existing `PyGraftSession`, activates
that session with one `PyGraftSessionOptions`, and gives the existing graft
receiver one Python nudge callback.

The adapter does not open a socket, write ATM data, retry a delivery, poll, or
implement an alternate acknowledgement path. It forwards a canonical nudge
body to the host's ordinary inbound-user-message callable exactly once for a
message ID retained in its bounded in-memory duplicate set.

## Typed callback

The callback receives `PyNudge` from AI.18. It uses only these structured
fields:

- `nudge.message_id` — immutable duplicate-suppression key;
- `nudge.source.agent`, `nudge.source.chat_id`, and `nudge.source.team` —
  chat identity inputs;
- `nudge.body` — the body submitted to Hermes's normal inbound-user-message
  path.

It never parses the rendered `agent:chat-id@team` form. For a source with a
chat ID it constructs `atm:agent:chat-id@team`; without one it constructs
`atm:agent@team`. The `atm:` prefix is reserved for ATM conversations and
keeps their state disjoint from Telegram, Discord, and every other host
namespace.

The callback is emitted only after the canonical write is durable. A failed
host injection propagates to the existing graft callback caller; the bridge
does not retry or persist a delivery result.

## Downstream handoff

Hermes maintainers copy or package this adapter in the Hermes repository,
connect `inject_user_message(chat_key, body)` to Hermes's existing inbound
user-message path, and validate it in that repository's normal review and
test process. This ATM-core sprint changes no external checkout and does not
make downstream Hermes merge status a closure gate.
