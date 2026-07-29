# Hermes Graft Adapter Contract

> **Status:** AI.38 target contract. The current checked-in bridge is generic
> `inject_user_message` callback plumbing; it does not yet contain the new
> `atm_graft_hermes_adapter.py` artifact or a Hermes steer implementation.

## Scope

`atm_graft_hermes_bridge.HermesGraftBridge` stays the generic ATM-core bridge:
it creates one `PyGraftSession`, activates one receiver, de-duplicates typed
nudges by bounded message-ID memory, and invokes its supplied callback. It
does not know Hermes steer semantics. AI.38 creates the separate
`atm_graft_hermes_adapter.py` artifact and its `HermesSteerPort`; that adapter
owns forwarding a live nudge or recovery notice to the configured profile's
non-interrupting steer callable. Neither layer opens an ATM socket directly,
writes/retains mail, retries delivery, or implements an alternate
acknowledgement path.

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

## Telegram routing

`HermesGraftBridge` emits the canonical ATM chat key to its host callback. The
Hermes `AtmGraftAdapter` must not use that key as a second Hermes conversation:
it creates an internal event on `Platform.TELEGRAM` with the configured
`ATM_CHAT_ID`, preserving the ATM key only as sender metadata. This is what
wakes the live Telegram session and sends the normal response back to Telegram.
`ATM_CHAT_ID` is required; a missing value fails closed and logs a routing
error instead of falling back to `Platform.LOCAL`.

## Downstream handoff

AI.38 supplies a checked-in `HermesSteerFixture` and
`scripts/phase-ai/run-hermes-steer-smoke.py` as its merge-gating reference
proof. Hermes maintainers may additionally copy or package the adapter in the
Hermes repository and connect its narrow `HermesSteerPort` to the supported
steer API. That downstream production merge is useful operational evidence but
is not a closure gate for the ATM-core sprint.
