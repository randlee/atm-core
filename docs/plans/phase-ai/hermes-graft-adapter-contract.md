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

## Adapter contract tests

`just test-hermes-graft-bridge` builds the Maturin extension in an isolated
virtual environment and runs both `test_hermes_bridge.py` and
`test_hermes_adapter.py`. The latter imports the checked-in, dependency-free
Hermes gateway contract shim at
`crates/atm-graft-python/tests/hermes_gateway_shim` so the adapter contract is
actually exercised in CI. To run the same tests against a Hermes checkout,
set `HERMES_SRC` to its repository root; it must contain
`gateway/platforms/base.py`. The shim is only a test harness and is not used
by the installed plugin.

## Typed callback

The callback receives `PyNudge` from AI.18. It uses only these typed values:

- `nudge.message_id` — immutable duplicate-suppression key;
- `nudge.source` — a validated `PyAgentAddress` whose `__str__()` renders the
  canonical chat identity;
- `nudge.body` — the body submitted to Hermes's normal inbound-user-message
  path.

`PyNudge(message_id, source, body)` is a validated Python value constructor
for bridge/reference-adapter tests. It validates the immutable message ID and
nonblank body; it does not write, route, or synthesize a nudge.

It performs no address parsing, segment validation, or rendering. It prefixes
the binding-rendered identity with `atm:`, yielding `atm:agent:chat-id@team`
or `atm:agent@team`. The prefix keeps ATM state disjoint from Telegram,
Discord, and every other host namespace.

The callback is emitted only after the canonical write is durable. A failed
host injection propagates to the existing graft callback caller; the bridge
does not retry or persist a delivery result.

## Downstream handoff

Hermes maintainers copy or package this adapter in the Hermes repository,
connect `inject_user_message(chat_key, body)` to Hermes's existing inbound
user-message path, and validate it in that repository's normal review and
test process. This ATM-core sprint changes no external checkout and does not
make downstream Hermes merge status a closure gate.
