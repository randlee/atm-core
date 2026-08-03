---
title: AK.5 Direct peer resend cache and timer aggregate
status: proposed
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.4
parallel_safe: false
---

# AK.5 — direct peer resend cache and timer aggregate

## Closure

Add optional default-on resend caching to AK.4's proven direct HTTP function.
It adds one endpoint aggregate and one timer, never a worker or alternate path.

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
```

`peer_resend_cache` defaults to `true`; it is stored in the one-row
`peer_delivery_settings(singleton PRIMARY KEY CHECK (singleton = 1),
resend_cache_enabled INTEGER NOT NULL DEFAULT 1)` table. The scheduler owns one
`PeerResendState` under one mutex; it stores no payload, request copy, ULID
list, agent/session state, or delivery result. The immutable `peerOutbound`
records remain the sole backlog. The mutex makes the admission transition,
`Disconnected` in-progress guard, and earliest-deadline update one atomic
state transition.

`Connected` performs AK.4's `send_peer_http_frames` immediately. A failure
sets `Queued { due_at >= now + 60s }`; new sends for a queued endpoint only
persist and return a pending-delivery error. When the due event begins its one
oldest-first batch, the endpoint is `Disconnected` so concurrent admissions
do not connect. Full success sets `Connected`; any failure returns it to
`Queued` with a new due time. `Disconnected` is only this in-progress guard,
not a health claim and not a fourth state.

There is no general timer service today. AK.5 extends the existing local IPC
accept loop with the scheduler's one `earliest_due` deadline. Before that
deadline, the loop does not scan resend state or SQLite; at the deadline it
calls `poll_due_peer_resends(now)`. It creates no timer thread, worker, task,
channel, or periodic resend polling loop. The callback chooses at most one due
endpoint per due event, reads one bounded oldest-first page from
`OutboundMessageQuery::page_for_peer`, and calls AK.4's same slice sender.
`PEER_RESEND_BATCH_LIMIT` is exactly `64`; its one conversion to `NonZeroU16`
for `page_for_peer` is checked. It bounds one direct batch, not the durable
backlog. An endpoint absent from
the map is optimistically `Connected`; no health state is persisted. On runtime
construction when caching is enabled, `bootstrap_pending_peer_resends` performs
one read-only `SELECT DISTINCT peerOutbound.host` query of already-undelivered
records. Its result has at most one row per configured canonical peer, not one
row per message; it creates `Queued` aggregates due no earlier than 60 seconds
later. It reads no payload and opens no connection. This is the only restart
recovery: it is not a worker, peer-config scan, or recurring database poll.
Disabled caching is exactly AK.4: failed messages remain undelivered and return
an error with no aggregate or due event.

## Type and boundary inventory

| Item | AK.5 role |
| --- | --- |
| `PeerConnectionState` | New three-value enum only: `Connected`, `Disconnected`, `Queued { due_at }`. It is endpoint transport state, never agent health. |
| `PeerResendAggregate` | New per-endpoint in-memory state: only `state`; `Queued` owns `due_at`, so connected/disconnected values cannot carry a meaningless deadline. It has no payload, ULID list, receipt, or peer scan result. |
| `PeerResendState` | New private mutex-protected aggregate map plus `earliest_due`. It is the sole atomic state owner; no separate lock or atomics may mirror its deadline. |
| `PeerResendScheduler::{deliver_or_queue, bootstrap_pending_peer_resends, poll_due_peer_resends, next_due}` | New owner of the one mutex-protected state, immutable AK.3 directory/AK.4 HTTP config, and existing sealed storage ports. These direct dependencies let both singleton and batch calls use AK.4's exact sender/confirmation without a new trait/service. `next_due` exposes the coalesced earliest deadline; `poll_due_peer_resends(now)` runs only at/after it and owns no thread/task/channel. |
| `Instant` | Existing monotonic clock value. It exists only inside `Queued { due_at }`; no wall-clock timestamp or transport health is persisted. |
| `PeerResendCacheSetting`, `peer_delivery_settings` | New persisted one-bit default-on setting and its exact one-row table. It replaces no old policy and has no per-peer override in this sprint. |
| `PeerConfigStore::{peer_resend_cache_setting, save_peer_resend_cache_setting}` | New sealed configuration accessors for the one setting. Saving uses the existing runtime-view reload; it does not start a worker or mutate a live endpoint map directly. |
| `PeerSubcommand::ResendCache`, `ResendCacheCommand` | New explicit CLI configuration surface for showing or setting the one Boolean. It owns no send/retry operation and has no per-peer form. |
| `PEER_RESEND_BATCH_LIMIT` | New exact `u16` bound of 64 for one oldest-first due batch; the sole `NonZeroU16` conversion occurs at `page_for_peer`. It is not an admission, connection, or retry-attempt cap. |
| `RuntimeServeHooks::{next_peer_resend_due, poll_due_peer_resends}` | Two new closure fields used by the existing local-IPC wait path: it caps the wait at the coalesced deadline and invokes the due callback only when due. They are not a new trait, thread, task, channel, or loop. |
| `OutboundMessageQuery::{pending_peer_endpoints, page_for_peer}` | Read-only immutable backlog readers. The former is one startup-only distinct-host query; the latter is the only payload read in a due batch. |
| `MessageStore::confirm_peer_delivery` | AK.4's existing sealed confirmation port, held only to retire each confirmed durable record after the shared sender returns its matching response. |
| `send_peer_http_frames` | AK.4 sender reused unchanged for singleton and batch delivery. |

No other retry struct, enum, trait, timer service, executor, worker, queue,
connection pool, or health model is authorized without a plan amendment.

## Deliverables

1. Add the exact three-value state and one aggregate per canonical endpoint;
   persist only the default-on `peer_resend_cache` setting. Do not reuse
   `PeerSyncPolicy`, add a per-message retry table, or store a second request.
   `Session`/agent/roster data is neither read nor written.
   Add only the listed peer-config accessors and `atm peer resend-cache
   {show,set <true|false>}`; a save uses the existing runtime-view reload. A
   reload builds one fresh scheduler from the current immutable directory,
   HTTP config, and setting: `false` drops all transient aggregates, while
   `true` performs the one bootstrap query. It does not mutate old scheduler
   state in place.
2. Add only the existing-accept-loop due callback described above. It must
   perform no DNS, peer-config scan, or resend work before `due_at`; one due
   callback may select only one endpoint and one bounded oldest-first page.
   Coalesce deadlines in `earliest_due`; do not call a resend callback on each
   1 ms `WouldBlock` pass.
3. Route immediate singleton and timer batch delivery exclusively through
   AK.4's `send_peer_http_frames`. A timer batch is a slice into that function,
   not a second transport or receiver path.
4. Keep resend state separate from agent/session/roster/nudge state. A
   receiver's nudge remains normal post-write behavior and is never used as a
   resend signal.
5. Update requirements/architecture/ADR language for optional resend caching;
   state that AK.6 deletes obsolete legacy support. Create `ADR-046` for the
   three-state, in-memory endpoint aggregate and one startup recovery query;
   revise `REQ-CORE-TRANSPORT-003` and `-003B` so immutable `peerOutbound`
   data remains the sole durable backlog while the aggregate is explicitly
   non-durable. Update `docs/adr/INDEX.md`, `docs/architecture.md`,
   `docs/boundaries.md`, `docs/atm-daemon/{architecture,boundaries,requirements}.md`,
   `docs/atm/{architecture,requirements}.md`, and `docs/peer-pair-smoke.md`.

## Explicit prohibitions

- No coordinator, timer thread, worker, task, per-message thread, channel,
  connection pool, global background scan, DNS thread, or peer-row scan.
- No retry when caching is disabled; no hidden automatic retry in AK.4.
- No decision based on agent state, session ID, roster state, or a nudge.

## Required validation

- Unit: `Connected` immediate singleton success; failure queues exactly one
  endpoint deadline; `Queued` adds no connection attempt; `Disconnected`
  prevents a concurrent attempt.
- Unit: due callback reads one oldest-first page and passes its ordered writes
  of at most `PEER_RESEND_BATCH_LIMIT` to AK.4's exact slice sender;
  partial/failure re-arms without duplicating durable records.
- Unit: a daemon restart with enabled caching performs one distinct-pending-host
  bootstrap, schedules no payload or connection before 60 seconds, and later
  uses the same due callback. With caching disabled, it performs no bootstrap,
  creates no aggregate, and does not retry a retained record.
- Migration: an existing database receives the one default-on settings row;
  an explicit `false` survives restart and creates no aggregate.
- CLI/integration: `atm peer resend-cache set false` persists the setting and
  the reloaded scheduler takes the no-retry path without daemon restart or new
  background work; toggling it back to `true` creates one fresh bootstrap map
  with no duplicate endpoint aggregate.
- Unit: disabled caching leaves no aggregate/due event and returns the AK.4
  delivery error.
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
merge. Push AK.5, then start AK.6 with AK.5→AK.6 merge-forward.
`must_follow` is required because AK.6 removes only code superseded by the
AK.4/AK.5 path; it is not parallel-safe because both touch peer transport.
