# ADR-043 — Hermes Graft Wake-up Ownership and Recovery

| Field | Value |
| --- | --- |
| ID | ADR-043 |
| Status | Accepted — AI.36 receiver ownership implemented |
| Scope | `atm-graft`, Python binding, and Hermes reference adapter |
| Relates to | ADR-037, ADR-039, ADR-033 |

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

## Consequences

- A profile restart becomes recoverable even when a prior live nudge was lost.
- A second live gateway cannot silently steal another profile's wake-ups.
- The host agent sees concise ATM work prompts in its safe steer flow and uses
  its normal ATM skills to inspect/ack durable mail.
- The implementation adds a small count projection over the existing daemon
  read contract; it adds no daemon session, mailbox table, queue, or protocol.
- An in-session steer failure remains operator-observable residual risk until a
  separately approved bounded recovery design exists.

## Alternatives considered

- **Normal Telegram-style `MessageEvent` injection:** rejected because it can
  disrupt a running agent and has different scheduling semantics than steer.
- **A graft-owned durable queue/retry store:** rejected because it duplicates
  daemon mailbox authority and creates reconciliation/state ownership.
- **Last writer wins endpoint publication:** rejected because it loses a live
  profile's wake-ups and permits an old close to delete a successor record.
- **Multi-chat endpoint registry now:** deferred; it adds policy/fan-out work
  before the required one-profile reliability path is proven.

## Follow-up work

- AI.36: complete — `GraftReceiverListener` holds an OS-backed exclusive
  lock, publishes a random generation plus optional `ChatId`, and uses
  generation-checked cleanup. Coverage includes typed live-owner conflict,
  stale-record replacement, and concurrent distinct identities.
- AI.37: one delayed durable-work summary after profile recovery.
- AI.38: Hermes steer injection and end-to-end non-interruption evidence.
