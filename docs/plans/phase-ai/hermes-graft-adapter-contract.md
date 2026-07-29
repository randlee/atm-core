# Hermes Graft Adapter Contract

> **Status:** AI.38 reference contract. The checked-in adapter binds one
> configured Hermes session to the documented non-interrupting steer seam.

## Scope

`atm_graft_hermes_bridge.HermesGraftBridge` stays the generic ATM-core bridge:
it creates one `PyGraftSession`, activates one receiver, de-duplicates typed
nudges by bounded message-ID memory, and invokes its supplied callback with
the original `PyNudge`. It does not know Hermes steer semantics.
`atm_graft_hermes_adapter.py` owns the narrow injected `HermesSteerPort`; it
forwards a live nudge or recovery notice to the configured profile's
non-interrupting steer callable. Neither layer opens an ATM socket directly,
writes/retains mail, retries delivery, or implements an alternate
acknowledgement path.

## Adapter contract tests

`just test-hermes-graft-bridge` builds the Maturin extension in an isolated
virtual environment and runs both `test_hermes_bridge.py` and
`test_hermes_adapter.py`. Those tests use only checked-in fake steer ports and
the checked-in `HermesSteerFixture`; they do not import a Hermes checkout or a
gateway shim. This makes the boundary, no-normal-ingress guard, and safe-tool
turn proof reproducible in CI. A downstream Hermes checkout may separately
bind `HermesRpcSteerPort` to its authenticated RPC client, but is operational
evidence rather than this sprint's merge gate.

## Supported Hermes steer contract

The installed Hermes source (`tui_gateway/server.py`) exposes the RPC method
`session.steer`. The port calls it as:

```json
{"method":"session.steer","params":{"session_id":"<ATM_CHAT_ID>","text":"<nonblank text>"}}
```

An accepted response contains `result.status == "queued"`; `"rejected"`, a
missing result, or an RPC `error` is surfaced as `HermesSteerFailure`. There
is deliberately no normal-message fallback. Hermes owns the scheduling rule:
the accepted text is visible at the next safe tool boundary without interrupt.

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
binding-rendered identity is retained as attribution (`agent:chat-id@team` or
`agent@team`); it does not select a second Hermes conversation. The configured
profile `ATM_CHAT_ID` selects the one current host session.

The callback is emitted only after the canonical write is durable. A failed
host injection propagates to the existing graft callback caller; the bridge
does not retry or persist a delivery result. After receiver activation, it may
perform the single ADR-043 ten-second count check and submit one advisory
recovery steer; this is derived from the ordinary daemon mailbox read contract
and is not a nudge queue or replay.

The recovery callback receives only `MailboxRecoveryNotice(unread,
pending_ack)`. It prompts the host to use normal ATM skills; it never exposes
message bodies or changes mailbox read/acknowledgement state.

## One-profile routing

`ATM_CHAT_ID` is required and is validated once when the adapter connects.
Every live nudge and AI.37 recovery summary uses that exact session id. A
source chat id remains attribution/reply-address data only; it cannot create a
second Hermes conversation, registry row, `MessageEvent`, or normal inbound
authorization path. A missing or blank configured id fails closed.

## Downstream handoff

AI.38 supplies a checked-in `HermesSteerFixture` and
`scripts/phase-ai/run-hermes-steer-smoke.py` as its merge-gating reference
proof. It records one `live_nudge` and one `recovery_summary`, with the
configured profile/session, accepted steer, and explicit non-interruption/no
mail-mutation evidence. Hermes maintainers may additionally connect the port
to a production RPC client. That downstream run is useful operational evidence
but is not a closure gate for the ATM-core sprint.
