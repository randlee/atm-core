# ADR-043 — Hermes Graft Wake-up Ownership and Recovery

| Field | Value |
| --- | --- |
| ID | ADR-043 |
| Status | Accepted — AI.36 receiver ownership implemented |
| Scope | `atm-graft`, Python binding, and Hermes reference adapter |
| Relates to | ADR-037, ADR-039, ADR-033 |

*Terminology note (Phase AQ): 'nudge' below means the steer (immediate) kind; see the nudge taxonomy in docs/requirements.md.*

## Context

ATM mail is durable in the daemon-owned mailbox. Graft exists to provide a
thin embedded daemon client and to push bounded wake-up notifications into a
host agent. The existing receiver record has one pathname per root/team/agent
but no ownership generation or safe hand-off rule. The reference Hermes
adapter also converts graft nudges into normal Telegram-style user messages,
which can interrupt a running agent.

Hermes runs one long-lived profile for each ATM team member. Profiles can
restart independently; a restart must discover durable work without making
graft a second mailbox. A profile's `ATM_CHAT_ID` identifies its actual host
session today. Future multi-channel support is desirable but is not required
to make the one-profile path reliable.

## Decision

1. A graft receiver endpoint is singly owned by `(canonical graft root, team,
   agent)`. Activation acquires an OS-backed ownership guard and publishes a
   fresh random generation in the endpoint record. Concurrent activation fails
   without mutating the active record. Close removes a record only when its
   generation matches the owner; process death releases the ownership guard so
   the next profile can reclaim it.
2. Graft's only persistent state is none. Its temporary state is bounded
   nudge-handoff work. The daemon mailbox and ordinary daemon API remain the
   sole authority for mail, unread state, and acknowledgement state.
3. A Hermes profile carries one configured `ChatId`. Live graft nudges and the
   one restart summary invoke Hermes's non-interrupting steer seam for that
   exact profile session. They must not use normal inbound-user-message
   dispatch and must not create a Hermes conversation manager.
4. Exactly ten seconds after a successful receiver activation, the Python
   adapter queries ordinary daemon `ReadOutcome.bucket_counts`. If `unread` or
   `pending_ack` is non-zero, it emits one advisory steer: `ATM: <u> unread
   messages; <p> acknowledgements pending.` It makes no individual message
   replay, read, ack, retry loop, or persistence write.
5. The endpoint record remains one receiver per agent for now. It stores the
   profile's `ChatId` for validation/observability but does not add multi-chat
   routing, fan-out, or a second endpoint namespace. A future multi-channel
   feature must explicitly revise this ADR.
6. A live steer injection failure while the receiver remains listening is
   logged and surfaced to the host, but has no in-session retry, periodic
   poll, normal-message fallback, or graft queue. The accepted recovery path
   for this first release is a subsequent profile restart/reconnect, which
   produces the one ten-second durable-mail summary. This residual is explicit
   rather than hidden by an unbounded background mechanism.
7. `hermes-atm` owns an optional, package-registered native-tool surface for
   the configured profile: initially `atm_send`, `atm_read`, and `atm_list`.
   Tool schemas and semantics are CLI-equivalent, but the configured profile
   remains the sole source of identity, team, ATM home, and workspace root.
   No tool parameter may replace that caller context.
8. Those tools use the public `atm_graft` Python API only. They neither run the
   CLI nor open a second transport/receiver, poll loop, retry queue, storage
   handle, or daemon lifecycle path. A thin addition to the public Python API
   is permitted only when it translates the same ordinary daemon operation
   used by the CLI.
9. Each tool returns a JSON-compatible discriminated union:
   `{"kind":"success","result":...}` or
   `{"kind":"error","error":{"code":...,"message":...,"recovery":...}}`.
   Unsupported host capability or registration failure is fail-closed and
   observable; it must never take down the gateway.
10. The JSON ingress boundary is owned by Pydantic v2 request models in the
    ATM Python layer. The handler validates incoming JSON once, translates
    once to typed graft calls, and directly serializes trusted typed outcomes.
    Result/error data is not re-validated in production. Unknown fields and
    invalid or mutating read requests fail before native transport.

## Consequences

- A profile restart becomes recoverable even when a prior live steer nudge was
  lost.
- A second live gateway cannot silently steal another profile's wake-ups.
- The host agent sees concise ATM work prompts in its safe steer flow and uses
  its normal ATM skills to inspect/ack durable mail.
- The implementation adds a small count projection over the existing daemon
  read contract; it adds no daemon session, mailbox table, queue, or protocol.
- An in-session steer failure remains operator-observable residual risk until a
  separately approved bounded recovery design exists.
- Tool use remains ordinary daemon API use, so mailbox authority and wake-up
  ownership do not move into the Hermes package.
- A single installed profile has one fixed caller context across receiver and
  tools, preventing a tool invocation from impersonating another team member.

## Alternatives considered

- **Normal Telegram-style `MessageEvent` injection:** rejected because it can
  disrupt a running agent and has different scheduling semantics than steer.
- **A graft-owned durable queue/retry store:** rejected because it duplicates
  daemon mailbox authority and creates reconciliation/state ownership.
- **Last writer wins endpoint publication:** rejected because it loses a live
  profile's wake-ups and permits an old close to delete a successor record.
- **Multi-chat endpoint registry now:** deferred; it adds policy/fan-out work
  before the required one-profile reliability path is proven.
- **Shelling out to `atm` from a Hermes tool:** rejected because it creates a
  second configuration/parsing boundary and cannot provide the typed,
  in-process error contract required of a host-native tool.

## Follow-up work

- AI.36: complete — `GraftReceiverListener` holds an OS-backed exclusive
  lock, publishes a random generation plus optional `ChatId`, and uses
  generation-checked cleanup. Coverage includes typed live-owner conflict,
  stale-record replacement, concurrent distinct identities, and a real
  child-process crash/reclaim proof executed by the macOS, Linux, and Windows
  CI test matrix.
- AI.37: complete — `MailboxWorkCounts` projects only existing daemon read
  buckets, while the generic bridge schedules one cancellable ten-second
  recovery callback. The callback has no message body, replay, persistence,
  acknowledgement, or retry behavior.
- AI.38: complete — live and delayed recovery wakes both use the injected
  `session.steer` port. The adapter resolves the configured platform
  `ATM_CHAT_ID` through an injected `resolve_session_id` callable and sends
  only the opaque Hermes runtime session ID; it fails closed when that binding
  is unavailable or invalid. The checked-in fixture proves accepted text
  appears only after a safe tool boundary, with no normal inbound-message
  call, interruption, or mailbox mutation. The adapter is a reference seam;
  no production Hermes caller currently instantiates the resolver. That
  wiring gap remains tracked separately as
  `AI3138-HERMES-NO-PRODUCTION-LOADER`.
