# ADR-028C — Cross-Host Delivery: Host-Tagged Messages, No Replay Store

| Field | Value |
| --- | --- |
| ID | ADR-028C |
| Status | **Accepted** (delivery mechanics have open items — see Deferred) |
| Date | 2026-07-20 |
| Deciders | Rand Lee |
| Relates to | ADR-028A (error consolidation), ADR-028B (closed protocol) |
| Supersedes | ADR-ATM-RUNTIME-001 §replay-store ownership |

## Context

A durable, SQLite-backed `RemoteReplayStore` was added to
`atm-runtime`/`atm-daemon` without ever being a stated requirement. It was
deleted once discovered, then rebuilt in full during a same-session context
loss — the deleting session's intent never survived into the next one,
because nothing in the repository recorded the deletion as permanent.

The boundary control meant to prevent this reintroduction was checked and
found to be non-functional:
`boundaries/atm-daemon/peer-client-transport.toml` declares
`io_forbidden = ["sqlite", "process_spawn"]`, but `sc-lint-boundary`'s
implemented rules (`ScbCycle001-003`, `ScbBoundary001-003`,
`ScbRuntime001-002`, `Port001-005`) never read the `io_forbidden` field at
all. The protection was documentation, not enforcement.

Separately, `boundaries/atm-runtime/runtime-composition.toml` currently
documents `replay_store_assembly` as an owned, allowed responsibility of
`atm-runtime` — this file needs updating to reflect this ADR.

The design went through three iterations before landing here (see
Alternatives), converging once it was recognized that a send-side durable
record has value independent of retry semantics, and that reusing existing
message storage is simpler than any purpose-built queue.

## Decision

**There is no replay store.** A cross-host message is an ordinary message,
tagged with its destination host, delivered through the existing nudge
interface.

### Storage: extend the message, don't build an outbox

| Need | Existing code found | Disposition |
| --- | --- | --- |
| Destination host on a message | Not present — `MessageEnvelope` (`atm-storage/src/schema/inbox_message.rs`) has no `host` field | **New.** Field to be added. |
| Team → host lookup | Not present — no `TeamHostMapping`, `HostRegistry`, or equivalent found anywhere in the workspace | **New.** Lookup to be added. |
| Staleness cutoff | **Already present**: `MessageEnvelope.expires_at: Option<IsoTimestamp>`, with a working comparison already in `threading.rs`: `expires_at.is_some_and(\|e\| e <= now)` | **Reused verbatim.** At send time, `expires_at = send_time + X`. No new field, no new comparison logic. |
| Local durable write on send | **Already present** — the existing atomic mailbox write path | **Reused, unchanged.** Every send writes locally first, cross-host or not. |

### Delivery: a routing branch on the nudge interface, not a new transport

| Existing type | Location | Role |
| --- | --- | --- |
| `HostNudgeInjector` trait: `fn inject_nudge(&self, nudge: &PostSendHookEvent) -> Result<(), AtmError>` | `atm-graft/src/lib.rs` | Already single-shot, fallible, no retry — the exact reliability contract this design needs. |
| `BuiltInNudgeSinkTarget` enum: `{ Tmux, Graft }` | `atm-core/src/boundary/mod.rs` | Existing sink polymorphism. |
| `InternalNudgeEnvelope { event, sink_target, template }` | `atm-core/src/boundary/mod.rs` | Existing envelope already carries structured content, not just a bare signal. |
| `LocalTmuxNudgeTarget { pane_id, rendered_nudge }`, `GraftNudgeTarget { recipient, recipient_team }` | `atm-core/src/boundary/mod.rs` | Existing per-sink target structs. |
| Dispatch site: `BuiltInNudgeSinkTarget::Tmux => TmuxNudgeSink.deliver(...)`, `::Graft => GraftNudgeSink.deliver(...)` | `atm/src/commands/internal_nudge.rs` | Where sink selection actually happens today — inside the `atm internal-nudge` CLI command, which acts on whatever host runs it. |

**Correction carried forward from design discussion:** cross-host delivery is
*not* a third `BuiltInNudgeSinkTarget` variant parallel to `Tmux`/`Graft`. Both
existing sinks act on the local host by construction. The actual new piece is
a routing decision one layer above sink dispatch: does this daemon invoke
`internal_nudge` itself (recipient is local), or make one call telling the
recipient's home-host daemon to invoke its own `internal_nudge` (recipient is
remote)? Same single-shot, `Result`-returning contract either way — nothing
about the reliability model changes, only the routing target.

`peer_transport.rs` (1,629 lines) is not ported. It is replaced by this
routing branch — the file's retry/backoff/replay-resume logic
(`INITIAL_RETRY_BACKOFF`, `MAX_RETRY_BACKOFF`, `DEFAULT_REMOTE_RETRY_BUDGET`,
`MAX_REMOTE_RETRY_BUDGET`, `MAX_REMOTE_REPLAY_RESUME_RECORDS`,
`resume_pending_replay()`) is deleted, not rewritten.

### Caller-facing behavior

1. `atm send` writes the message locally first (existing atomic write,
   unchanged), tagged with its destination host if cross-host.
2. Delivery is attempted immediately, up to ~1 second.
   - Resolves within that window → caller gets `success`, synchronously.
   - Does not resolve within that window → caller gets `attempting to send`
     and the call returns.
3. If a message that returned `attempting to send` later succeeds, the
   sender is told **as a message** — a receipt lands in their own mailbox via
   the same message-delivery mechanism, not a side channel.

### Sync trigger: host-connected event, not a timer

Delivery is not retried on a schedule or backoff. It is retried on a
**host-connected event**: when the daemon successfully connects to a
previously-unreachable remote host, it walks locally-stored messages tagged
for that host that are not yet synced, and sends every one where
`now < expires_at` (the existing `threading.rs` check). Any tagged message
where `now >= expires_at` is simply excluded from that batch — not sent, no
separate failure/error path fires for it.

**Verified: no host-connected concept exists anywhere in this codebase
today** (`host_connected`, `HostConnected`, `peer_online`, `PeerOnline`,
`host_reachable`, `presence` — no matches). This is new capability, not a
rediscovery of something already built.

### X (the staleness window) is caller-specified per send

Not a system constant. Default is short (~1 hour — "expect the host back
soon"). A caller who knows the destination will be offline for an extended
period (e.g. a planned 48-hour outage) can set a longer X for that specific
send.

### Two usage shapes

1. **Now** — this ADR's scope. Deliver as soon as possible; retried only via
   host-connected events; governed by `expires_at`.
2. **Whenever** — noted as a generalization (a general offline task queue,
   no urgency), not designed here. See Deferred.

## Deferred / open items

1. **Host-connected detection mechanism** — is this the daemon passively
   noticing a peer became reachable, or a disguised poll (daemon
   periodically attempts reconnection, success fires the event)? Undecided.
2. **"Whenever" mode's staleness semantics** — unbounded wait, or an outer
   cap so records can't accumulate forever? Undecided.
3. **Pruning of expired, never-delivered "Now" records.** A message that
   ages past its `expires_at` before ever syncing will fail that same check
   on every future host-connected event too, since it only gets older.
   Nothing currently prunes it. Known gap, not solved here.
4. **The nudge interface's name.** Flagged by the decider as no longer quite
   right, now that it covers cross-host message delivery and not only
   in-session wake signals. Rename not yet chosen.
5. `runtime-composition.toml`'s documentation of `replay_store_assembly` as
   an owned responsibility needs updating to match this ADR.

## Enforcement

`sc-lint-boundary` should be extended to actually enforce `io_forbidden` —
currently declared, never read by any implemented rule. This is the
mechanical backstop that should have caught the original replay-store
reintroduction and did not; fixing it protects every boundary file's
`io_forbidden` list in the repository, not only this one.

## Alternatives considered

- **Durable SQLite replay store, 7-day retry budget** (original/reverted
  design). Rejected: durability across daemon restarts and multi-day retry
  were never stated requirements; 1-hour-late delivery was already called
  out as unacceptable ("stale/incorrect information").
- **In-memory FIFO retry queue, ~5 minute timeout, no persistence.**
  Considered as a middle ground between the two extremes. Superseded once it
  became clear the send-side durable record has independent value
  (audit/history) regardless of retry semantics, and that reusing existing
  message storage is simpler than a purpose-built queue.
- **Single HTTP POST, fail immediately on any failure, no state at all.**
  Considered as the simplest possible version ("if post fails, done").
  Superseded once "no reason to remove a send-side durable record, if
  added" was established — the record turned out to be worth keeping on its
  own merits, which reopened the design toward sync-on-reconnect rather than
  pure fire-and-forget.

## Consequences

- No standalone peer-transport subsystem; cross-host delivery is a routing
  branch on the existing, already-tested nudge path.
- Daemon startup no longer depends on replay-store assembly — that
  fail-closed path is deleted along with the store.
- A new `host` field and team→host lookup must be built; these are the only
  genuinely new pieces of state in this design — everything else reuses
  existing code.
