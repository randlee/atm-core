---
title: AK.5 Direct peer resend cache and timer aggregate
status: proposed
branch: feature/pak-s5-direct-peer-timer-state
worktree: ../atm-core-worktrees/feature/pak-s5-direct-peer-timer-state
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.4
parallel_safe: false
---

# AK.5 — direct peer resend cache and timer aggregate

## Closure

Add optional, disableable resend caching to AK.4's proven direct HTTP
function. It defaults off: undelivered messages simply remain in the
immutable database backlog unless an operator explicitly enables caching. It
adds one endpoint aggregate and one timer, never a worker or alternate path.

## Fixed contract

```rust
enum PeerConnectionState {
    Connected,
    Disconnected,
    Queued { due_at: Instant },
}

struct PeerResendAggregate {
    state: PeerConnectionState,
}

struct PeerResendState {
    aggregates: HashMap<PeerEndpoint, PeerResendAggregate>,
    earliest_due: Option<Instant>,
}

const PEER_RESEND_BATCH_LIMIT: u16 = 64;
const PEER_RESEND_DUE_CALLBACK_BUDGET: Duration = Duration::from_millis(250);
const PEER_RESEND_RETRY_DELAY: Duration = Duration::from_secs(60);

enum LocalIpcWaitOutcome {
    Accepted(LocalSocketStream),
    DeadlineElapsed,
}

fn accept_until(
    listener: &LocalSocketListener,
    deadline: Option<Instant>,
) -> Result<LocalIpcWaitOutcome, AtmError>;

struct PeerResendCacheSetting {
    enabled: bool,
}

struct PeerResendScheduler {
    state: Mutex<PeerResendState>,
    setting: PeerResendCacheSetting,
    http: PeerHttpRuntimeConfig,
    directory: PeerDirectory,
    outbound: Arc<dyn OutboundMessageQuery + Send + Sync>,
    messages: Arc<dyn MessageStore + Send + Sync>,
}

impl PeerResendScheduler {
    fn deliver_or_queue(
        &self,
        endpoint: PeerEndpoint,
        write: WriteRequest,
        deadline: RequestDeadline,
    ) -> Result<(), AtmError>;
    fn bootstrap_pending_peer_resends(&self) -> Result<(), AtmError>;
    fn next_due(&self) -> Option<Instant>;
    fn poll_due_peer_resends(&self, now: Instant) -> Result<(), AtmError>;
}

trait OutboundMessageQuery {
    fn pending_peer_hosts(&self, budget: Duration) -> Result<Vec<HostName>, AtmError>;
    fn page_for_peer(
        &self,
        peer: &HostName,
        after: Option<(IsoTimestamp, AtmMessageId)>,
        limit: NonZeroU16,
        budget: Duration,
    ) -> Result<Vec<StoredPeerWrite>, AtmError>;
}
```

`peer_resend_cache` defaults to `false`; it is stored in the one-row
`peer_delivery_settings(singleton PRIMARY KEY CHECK (singleton = 1),
resend_cache_enabled INTEGER NOT NULL DEFAULT 0)` table. Disabled caching is a
first-class fast path, not a degraded mode of the scheduler: with `false`,
`deliver_or_queue` calls AK.4's `send_peer_http_frames` directly and returns
its result unchanged, acquiring no scheduler mutex, computing no deadline,
issuing no backlog query, and performing no retry. The `PeerResendScheduler`
and its `PeerResendState` exist and are exercised only when caching is
enabled; they are a separate, additive concern layered on top of the AK.4
path, never a precondition for it. When enabled, the scheduler owns one
`PeerResendState` under one mutex; it stores no payload, request copy, ULID
list, agent/session state, or delivery result. The immutable `peerOutbound`
records remain the sole backlog in both modes. The mutex makes the admission
transition, `Disconnected` in-progress guard, and earliest-deadline update one
atomic state transition.

`Connected` performs AK.4's `send_peer_http_frames` immediately. A failure
sets `Queued { due_at = now + PEER_RESEND_RETRY_DELAY }`; new sends for a queued endpoint only
persist and return a pending-delivery error. When the due event begins its one
oldest-first batch, the endpoint is `Disconnected` so concurrent admissions
do not connect. Full success sets `Connected`; any failure returns it to
`Queued` with a new due time. `Disconnected` is only this in-progress guard,
not a health claim and not a fourth state.

With caching enabled, a first `Connected`-state attempt that fails queues the
endpoint **and returns the exact ordinary AK.4 delivery `AtmError`** to the
initiating caller. It is never converted to success or to a masked
"accepted for retry" result. A later admission while already `Queued` makes
no connection attempt, persists its immutable write, and returns a typed
pending-delivery error. Both errors state that local persistence succeeded;
neither emits an origin nudge.

There is no general timer service today. AK.5 extends the existing local IPC
serve loop with the scheduler's one `earliest_due` deadline because that loop
already owns daemon lifetime, shutdown, and runtime-view reload. The peer HTTP
listener remains inbound-only and must not initiate outbound delivery.
`accept_until` replaces an unbounded accept wait in that existing loop: it
returns `DeadlineElapsed` no earlier than its monotonic deadline or
`Accepted` when a local client arrives. It is a deadline-aware wait in the
existing loop, not a timer thread, worker, task, channel, or polling loop.

At each loop turn, `poll_due_peer_resends(now)` selects at most one due
endpoint by `(due_at, canonical_host, port)`, reads one bounded oldest-first
page from `OutboundMessageQuery::page_for_peer`, and calls AK.4's same slice
sender with a fresh internal `PEER_RESEND_DUE_CALLBACK_BUDGET` of 250 ms.
This is the only bounded work permitted in the serve loop after a deadline;
the normal immediate AK.4 path retains its originating request deadline.
After every callback the scheduler recomputes `earliest_due`. If another
endpoint is already due, the next loop turn has a zero-duration wait and
drains that endpoint before a later wait; otherwise the loop accepts normally.
This one-endpoint-per-turn order prevents a failing endpoint from starving
other due endpoints or local admissions, without creating a coordinator.
`PEER_RESEND_BATCH_LIMIT` is exactly `64`; its one conversion to `NonZeroU16`
for `page_for_peer` is checked. It bounds one direct batch, not the durable
backlog. An endpoint absent from
the map is optimistically `Connected`; no health state is persisted. On runtime
construction when caching is enabled, `bootstrap_pending_peer_resends` performs
one read-only `SELECT DISTINCT peerOutbound.host` query of already-undelivered
records. For each returned canonical `HostName`, it calls
`PeerDirectory::endpoint_for_canonical_host`; only `Some(PeerEndpoint)` creates
one `Queued` aggregate due no earlier than 60 seconds later. `None` means the
host is no longer configured: retain its durable record untouched, log the
configuration mismatch, and create no aggregate, guessed port, DNS lookup, or
connection. A later configuration reload builds a fresh scheduler and performs
the same one bootstrap query, allowing a restored configured endpoint to queue
normally. This is the only restart recovery: it is not a worker, peer-config
scan, or recurring database poll.
Disabled caching is exactly AK.4's direct call path: failed messages remain
undelivered and return an error with no aggregate, due event, mutex
acquisition, or backlog query performed on the way there.

## Type and boundary inventory

| Item | AK.5 role |
| --- | --- |
| `PeerConnectionState` | New three-value enum only: `Connected`, `Disconnected`, `Queued { due_at }`. It is endpoint transport state, never agent health. |
| `PeerResendAggregate` | New per-endpoint in-memory state: only `state`; `Queued` owns `due_at`, so connected/disconnected values cannot carry a meaningless deadline. It has no payload, ULID list, receipt, or peer scan result. |
| `PeerResendState` | New private mutex-protected aggregate map plus `earliest_due`. It is the sole atomic state owner; no separate lock or atomics may mirror its deadline. |
| `PeerResendScheduler::{deliver_or_queue, bootstrap_pending_peer_resends, poll_due_peer_resends, next_due}` | New owner of the one mutex-protected state, immutable AK.3 directory/AK.4 HTTP config, and existing sealed storage ports. These direct dependencies let both singleton and batch calls use AK.4's exact sender/confirmation without a new trait/service. `next_due` exposes the coalesced earliest deadline; `poll_due_peer_resends(now)` chooses one due endpoint deterministically and owns no thread/task/channel. |
| `Instant` | Existing monotonic clock value. It exists only inside `Queued { due_at }`; no wall-clock timestamp or transport health is persisted. |
| `PeerResendCacheSetting`, `peer_delivery_settings` | New persisted one-bit default-off setting and its exact one-row table. It replaces no old policy and has no per-peer override in this sprint. |
| `PeerConfigStore::{peer_resend_cache_setting, save_peer_resend_cache_setting}` | New sealed configuration accessors for the one setting. Saving uses the existing runtime-view reload; it does not start a worker or mutate a live endpoint map directly. |
| `PeerSubcommand::ResendCache`, `ResendCacheCommand` | New explicit CLI configuration surface for showing or setting the one Boolean. It owns no send/retry operation and has no per-peer form. |
| `PEER_RESEND_BATCH_LIMIT`, `PEER_RESEND_DUE_CALLBACK_BUDGET`, `PEER_RESEND_RETRY_DELAY` | New exact bounds: 64 writes for one oldest-first due batch, 250 ms for one due callback, and 60 s from delivery failure to the next eligible attempt. The sole `NonZeroU16` conversion occurs at `page_for_peer`. Neither is an admission, connection, or retry-attempt cap. |
| `LocalIpcWaitOutcome`, `accept_until` | New deadline-aware operation inside the existing local IPC serve loop. It returns one accepted local stream or one elapsed monotonic deadline; it adds no thread, timer service, channel, or second event loop. |
| `RuntimeServeHooks::{next_peer_resend_due, poll_due_peer_resends}` | Two new closure fields used by the existing local-IPC wait path: `next_peer_resend_due` supplies `accept_until`'s cap and `poll_due_peer_resends` runs one due endpoint after `DeadlineElapsed`. They are not a new trait, thread, task, channel, or loop. |
| `PeerDirectory::endpoint_for_canonical_host` | AK.3 bootstrap-only lookup from durable canonical host to its current `PeerEndpoint`, including port. It returns `None` for a deleted/disabled host and never guesses a port or uses DNS. |
| `OutboundMessageQuery::{pending_peer_hosts, page_for_peer}` | Read-only immutable backlog readers. `pending_peer_hosts` is the one startup-only `SELECT DISTINCT peerOutbound.host` query. AK.5 changes `page_for_peer` to remove the retired `not_before` age filter: it pages every still-undelivered record for one canonical host in `(message_at, message_id)` order. It is the only payload read in a due batch. |
| `MessageStore::confirm_peer_delivery` | AK.4's existing sealed confirmation port, held only to retire each confirmed durable record after the shared sender returns its matching response. |
| `send_peer_http_frames` | AK.4 sender reused unchanged for singleton and batch delivery. |

No other retry struct, enum, trait, timer service, executor, worker, queue,
connection pool, or health model is authorized without a plan amendment.

## Deliverables

1. Add the exact three-value state and one aggregate per canonical endpoint;
   persist only the default-off `peer_resend_cache` setting. Do not reuse
   `PeerSyncPolicy`, add a per-message retry table, or store a second request.
   `Session`/agent/roster data is neither read nor written.
   Add only the listed peer-config accessors and `atm peer resend-cache
   {show,set <true|false>}`; a save uses the existing runtime-view reload. A
   reload builds one fresh scheduler from the current immutable directory,
   HTTP config, and setting: `false` drops all transient aggregates, while
   `true` performs the one bootstrap query. It does not mutate old scheduler
   state in place.
2. Add only the deadline-aware existing-accept-loop callback described above.
   `accept_until` waits for a local connection or the one coalesced monotonic
   deadline; it must perform no DNS, peer-config scan, or resend work before
   `due_at`. One due callback selects only one deterministic endpoint and one
   bounded oldest-first page, with the exact 250 ms callback budget. Recompute
   `earliest_due` after it so already-due peers drain one per loop turn; do not
   call a resend callback on each 1 ms `WouldBlock` pass.
3. Replace `OutboundMessageQuery::page_for_peer`'s retired `not_before`
   parameter and SQLite `message_at >=` predicate with the exact cursor-only
   contract above; add the exact `pending_peer_hosts` distinct-host reader.
   This query change is solely AK.5 durable backlog selection: no age policy,
   peer scan, delivery mutation, or new table is allowed.
4. Route immediate singleton and timer batch delivery exclusively through
   AK.4's `send_peer_http_frames`. A timer batch is a slice into that function,
   not a second transport or receiver path.
5. Keep resend state separate from agent/session/roster/nudge state. A
   receiver's nudge remains normal post-write behavior and is never used as a
   resend signal.
6. Bootstrap only through `pending_peer_hosts` followed by
   `PeerDirectory::endpoint_for_canonical_host`. A durable host absent from
   the current directory remains untouched and unqueued; it is retried only
   if a later configuration reload restores a matching configured endpoint.
   Never infer its port, scan peers, or consult DNS.
7. Update requirements/architecture/ADR language for optional resend caching;
   state that AK.6 preserves only an isolated inactive interop fixture. Create `ADR-046` for the
   three-state, in-memory endpoint aggregate and one startup recovery query;
   revise `REQ-CORE-TRANSPORT-003` and `-003B` so immutable `peerOutbound`
   data remains the sole durable backlog while the aggregate is explicitly
   non-durable. Update `docs/adr/INDEX.md`, `docs/architecture.md`,
   `docs/atm-storage/boundaries.md`,
   `docs/atm-daemon/{architecture,boundaries,requirements}.md`,
   `docs/atm/{architecture,requirements}.md`, and `docs/peer-pair-smoke.md`.

## Explicit prohibitions

- No coordinator, timer thread, worker, task, per-message thread, channel,
  connection pool, global background scan, DNS thread, or peer-row scan.
- No retry when caching is disabled; no hidden automatic retry in AK.4.
- No decision based on agent state, session ID, roster state, or a nudge.

## Required validation

- Unit: `Connected` immediate singleton success; its first failure queues
  exactly one endpoint deadline and returns the same AK.4 delivery error;
  `Queued` adds no connection attempt and returns typed pending-delivery;
  `Disconnected` prevents a concurrent attempt.
- Unit: due callback reads one oldest-first page and passes its ordered writes
  of at most `PEER_RESEND_BATCH_LIMIT` to AK.4's exact slice sender;
  partial/failure re-arms without duplicating durable records.
- Storage integration: `page_for_peer` selects a canonical host's complete
  undelivered backlog in `(message_at, message_id)` order, including records
  older than the retired worker's former age window; cursor continuation has
  no duplicate or gap. `pending_peer_hosts` is distinct, deterministic, and
  read-only.
- Unit: a daemon restart with enabled caching performs one distinct-pending-host
  bootstrap, derives every queued endpoint and port only through
  `endpoint_for_canonical_host`, schedules no payload or connection before 60
  seconds, and later uses the same due callback. An absent configured host
  stays durable-but-unqueued with no guessed port or DNS lookup. With caching
  disabled, it performs no bootstrap, creates no aggregate, and does not retry
  a retained record.
- Migration: an existing database receives the one default-off settings row;
  it creates no aggregate until an explicit `true` is set, which then survives
  restart and performs the one bootstrap query.
- CLI/integration: `atm peer resend-cache set false` persists the setting and
  the reloaded scheduler takes the no-retry path without daemon restart or new
  background work; toggling it back to `true` creates one fresh bootstrap map
  with no duplicate endpoint aggregate.
- Unit: disabled caching leaves no aggregate/due event and returns the AK.4
  delivery error.
- Unit/integration: `accept_until` never invokes resend before `due_at`, emits
  `DeadlineElapsed` at the coalesced monotonic deadline, and processes due
  endpoint ties by `(due_at, canonical_host, port)` one per loop turn. A
  250-ms stalled due callback neither creates a thread nor prevents the next
  local accepted connection from being dispatched.
- Source gate: no coordinator, worker, timer thread, per-message thread,
  channel, immediate SQLite reload, DNS thread, or peer scan exists.
- Integration: immediate and timer resend use the same receiver/nudge path.
- Integration: each non-error batch response invokes AK.4's exact
  `confirm_peer_delivery`; a partial response leaves only unconfirmed writes
  eligible for the next oldest-first batch.
- Integration: successful immediate delivery, failed-then-recovered delivery,
  and a restarted queued backlog all use the same receiver persistence and
  post-write nudge path exactly once per accepted ULID. A timeout, malformed
  response, duplicate response, or nudge emission failure never becomes a
  resend-success signal.
- Smoke: after local `just smoke localhost` and `just smoke local-ip`, run
  isolated M4→M5 and M5→M4 `crosshost-send`, `crosshost-ack`, and
  `crosshost-curl-plain` lanes. For enabled caching, induce one connection
  failure, wait for the documented due event, and prove one recovered remote
  read plus one receiver nudge; for disabled caching, prove the record remains
  undelivered and no automatic resend occurs.
- `just lint` and `just test` pass.

## Dependencies

Before every AK.5 development/fix round, merge AK.4 into AK.5. Start AK.5 as
soon as AK.4 is pushed; do not wait for QA. AK.5 PR completion waits for AK.4
merge. AK.6 may already be developing its isolated fixture work after the
Phase AI entry gate; after AK.5 is pushed, it must merge AK.5 before its final
validation, final fix round, or PR completion.
`must_follow` is required because AK.5 retries AK.4's one verified function;
AK.6 finalizes its isolated inactive interop fixture only after that merge gate.
