# Hermes Graft Adapter Contract

> **Status:** AI.38 target contract. The current checked-in bridge is generic
> `inject_user_message` callback plumbing; it does not yet contain the new
> `atm_graft_hermes_adapter.py` artifact or a Hermes steer implementation.

## Scope

`atm_graft_hermes_bridge.HermesGraftBridge` is the ATM-core reference adapter
for one Hermes profile. It creates one existing `PyGraftSession`, activates
that session with one `PyGraftSessionOptions`, and gives the existing graft
receiver one Python nudge callback.

The adapter does not open a socket, write ATM data, retain mail, retry a
delivery, or implement an alternate acknowledgement path. It forwards a
canonical nudge body to the configured profile's non-interrupting steer
callable exactly once for a message ID retained in its bounded in-memory
duplicate set. ADR-043 and AI.36–AI.38 supersede the earlier ordinary
inbound-user-message handoff.

## Typed callback

The callback receives `PyNudge` from AI.18. It uses only these typed values:

- `nudge.message_id` — immutable duplicate-suppression key;
- `nudge.source` — a validated `PyAgentAddress` whose `__str__()` renders the
  canonical chat identity;
- `nudge.body` — the concise text submitted to Hermes's configured steer path.

`PyNudge(message_id, source, body)` is a validated Python value constructor
for bridge/reference-adapter tests. It validates the immutable message ID and
nonblank body; it does not write, route, or synthesize a nudge.

It performs no address parsing, segment validation, or rendering. The
binding-rendered identity is retained as attribution (`atm:agent:chat-id@team`
or `atm:agent@team`); it does not select a second Hermes conversation. The
configured profile `ATM_CHAT_ID` selects the one current host session.

The callback is emitted only after the canonical write is durable. A failed
host injection propagates to the existing graft callback caller; the bridge
does not retry or persist a delivery result. After receiver activation, it may
perform the single ADR-043 ten-second count check and submit one advisory
recovery steer; this is derived from the ordinary daemon mailbox read contract
and is not a nudge queue or replay.

## Downstream handoff

AI.38 supplies a checked-in `HermesSteerFixture` and
`scripts/phase-ai/run-hermes-steer-smoke.py` as its merge-gating reference
proof. Hermes maintainers may additionally copy or package the adapter in the
Hermes repository and connect its narrow `HermesSteerPort` to the supported
steer API. That downstream production merge is useful operational evidence but
is not a closure gate for the ATM-core sprint.
