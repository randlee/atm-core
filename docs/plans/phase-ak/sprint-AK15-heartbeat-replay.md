---
title: AK.15 Optional heartbeat-triggered replay
status: deferred_pending_AK13_AK14_acceptance
branch: feature/pak-s15-heartbeat-replay
worktree: ../atm-core-worktrees/feature/pak-s15-heartbeat-replay
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.14 merged to integrate/phase-ak and AK.13 physical proof accepted
merge_gate: explicit operator approval after minimal direct-path acceptance
parallel_safe: false
quality_findings: []
---

# AK.15 — optional heartbeat-triggered replay

## Preconditions

Do not start until the minimal direct design is accepted: AK.12's deletion
guards are merged, AK.13 has physically proven an outage and restoration
produce no automatic replay, and AK.14 has recorded that baseline in the
requirements and QA checklist. This proves replay is an intentional optional
extension, not hidden behavior required for basic cross-host delivery.

## Fixed architecture

AK.15 adds one optional, default-off policy:

```text
direct send ──success──► confirm exact singleton marker
     │
     └──failure──► durable peerOutbound marker remains
                              │
existing serial heartbeat ────┴──► healthy event
                                      │
                                      ▼
                         one bounded messages[] page
                                      │
                         one whole-page success
                                      ▼
                         confirm exact page markers
```

It does not add a transport. Direct sends remain singleton
`RequestEnvelope::Write` requests, exactly as in AK.11. In particular, the
AK.11 baseline does **not** send `messages[]`; `messages[]` is introduced only
by this explicitly enabled AK.15 recovery path.

The recovery page uses the same canonical `/v1/atm/messages` endpoint, shared
HTTP writer/reader, request decoder, `ApiRouter::route`, durable write, and
post-persistence received-message hook as a singleton. The sole receive-side
extension is a canonical array request value that normalizes an ordered array
into the same write-admission operation. It is not peer-only: it has no peer
decoder, peer route, peer listener, or peer persistence method.

## Configuration and durable data

- Introduce `peer_heartbeat_replay`, default `false`. Enabling it requires an
  explicit configuration write and normal daemon runtime-view reload. Do not
  revive or reinterpret the retired `peer_resend_cache` setting; its
  compatibility surface remains false-only. On daemon start or an enabled
  configuration reload, query only whether pending rows exist: initialize
  `Idle` when none exist and `BlockedUntilUnhealthy` when any exist. Thus
  enabling the option never flushes old rows until a new unavailable→healthy
  heartbeat transition has been observed.
- The existing `peerOutbound` rows are the sole durable pending cursor. The
  cursor key is canonical peer host plus ordered message identity; no payload
  cache, queue table, or second per-peer health map is introduced.
- Reuse the established `HostName` and `AtmMessageId` types everywhere in the
  cursor, state machine, API, tests, and events. AK.15 must not introduce
  `CanonicalHost`, `MessageId`, or another equivalent wrapper.
- Add one indexed storage operation that returns the oldest pending rows for a
  host, in deterministic cursor order, capped at the shared
  `MAX_MESSAGE_ARRAY_ITEMS` bound. That exact bound is used by both array
  decode and cursor-page selection; there is no second peer-specific limit.
- Add `confirm_peer_delivery_page(host, submitted_ids)` as the distinct
  AK.15-only transaction:
   it succeeds only when the submitted IDs exactly match the still-pending page
   for that host, and retires all of them together. No response/error path may
   partially advance the cursor.
- Bound durable replay backlog per `HostName` with one documented constant,
  `MAX_PENDING_PEER_OUTBOUND_PER_HOST`. There is no expiry, eviction, or
  silent drop. When enabled replay would add a marker beyond that cap, reject
  the host-qualified send before persistence with typed
  `PeerReplayBacklogFull`; surface the cap and current count in its diagnostic
  event. This is explicit backpressure, not a second queue.

## State machine

There is one serial heartbeat callback path; it is the timer. Before coding,
AK.15 must identify the exact existing heartbeat driver module and callback
symbol, cite its serialization contract in the PR, and add an executable test
of that contract. If no existing driver gives that guarantee, AK.15 is blocked
for operator direction; it may not add a worker, scheduler, coordinator,
second timer, channel, or per-peer task. The existing heartbeat service
retains only this small enum for each `HostName`; its existing health status
remains the source of unavailable→healthy events. The enum carries no payload,
cursor, timer, or connection state; the durable cursor remains the database
rows above.

```rust
enum HeartbeatReplayState {
    Disabled,
    Idle,
    AwaitingRecovery,
    Draining,
    Recovering,
    BlockedUntilUnhealthy,
}

fn record_direct_failure(&mut self, host: HostName);
fn on_peer_heartbeat(&mut self, host: HostName, event: HeartbeatEvent);
```

`record_direct_failure` only changes `Idle` to `AwaitingRecovery` when the
policy is enabled; it never sends. `on_peer_heartbeat` is the only function
allowed to send a replay page, and it can make at most one outbound call.

| State | Entry | Heartbeat event | Action | Next state |
| --- | --- | --- | --- | --- |
| `Disabled` | `peer_heartbeat_replay = false` | any | Do not query pending rows or send recovery traffic. | `Disabled` |
| `Idle` | enabled, no pending cursor page | any | Do not query or send recovery traffic. | `Idle` |
| `AwaitingRecovery` | enabled with pending rows after a direct-send failure, or after heartbeat observed unavailable | unhealthy | No send and no cursor change. | `AwaitingRecovery` |
| `AwaitingRecovery` | same | healthy | This is the unavailable→healthy recovery event: read exactly one oldest bounded page and send it once under `PEER_HTTP_LOCAL_RESPONSE_BUDGET`. | `Draining` while the bounded request is in flight |
| `Draining` | one page submitted | whole-page accepted response before deadline | Atomically confirm that exact page. Do not send another page inline. | `Idle` if no rows remain; otherwise `Recovering` |
| `Draining` | one page submitted | transport, deadline, protocol/validation, or non-success response | Do not alter the cursor; record the specific diagnostic code. | `BlockedUntilUnhealthy` |
| `Recovering` | a successful page left more pending rows | next healthy heartbeat | Read and send one next oldest bounded page. | `Draining` |
| `Recovering` | same | unhealthy heartbeat | No send and no cursor change. | `AwaitingRecovery` |
| `BlockedUntilUnhealthy` | a replay page failed while heartbeat was healthy | healthy | Do not resend the failed page. | `BlockedUntilUnhealthy` |
| `BlockedUntilUnhealthy` | same | unhealthy | Record the new unavailable observation. | `AwaitingRecovery` |

Thus the first healthy heartbeat after an outage starts recovery, and each
later *healthy heartbeat* drains at most one **new** page. The loop never
retries a failed page on the same tick, does not retry it on further healthy
heartbeats, and never spins or sends on an unrelated timer. A successful page
may be followed only by the next page at a later heartbeat; a failure requires
another observed unavailable→healthy transition.

At heartbeat entry, one absolute `RequestDeadline` is established and
propagated through the complete replay connect/write/read attempt; it is capped
by `PEER_HTTP_LOCAL_RESPONSE_BUDGET`. AK.15 introduces no separate per-stage,
longer, or unbounded heartbeat timeout. A heartbeat tick arriving while
`Draining` is outstanding performs no query and no send, records
`ReplayTickSkippedInFlight`, and leaves state unchanged. The cited
serial-driver contract should make that condition unreachable in production;
the rule prevents a future driver change from submitting the same page twice.

Direct-send event rules are fixed: a direct success never changes replay state;
a direct failure changes `Idle` to `AwaitingRecovery`; and a failure while
already `AwaitingRecovery`, `Recovering`, or `BlockedUntilUnhealthy` only adds
its durable marker to the ordered cursor. Disabling the option forces
`Disabled` and makes subsequent heartbeat events no-ops for replay.

Fresh direct sends never enter this state machine on success. On a direct
failure, the already-durable `peerOutbound` marker becomes eligible for the
next heartbeat only when the policy is enabled. No client waits for replay and
no direct request is transformed into an array.

## Canonical array admission and result rules

1. Define exactly this canonical protocol form; it replaces no singleton
   variant and has no peer-prefixed alternate:

   ```rust
   pub struct WriteBatchRequest {
       pub messages: Vec<WriteRequest>,
   }

   pub enum RequestEnvelope {
       Write(Box<WriteRequest>),
       Writes(Box<WriteBatchRequest>),
       // existing non-write variants unchanged
   }

   pub struct BatchSendOutcome {
       pub message_ids: Vec<AtmMessageId>, // exact submitted order
       pub warnings: Vec<WarningEntry>,
   }
   ```

   `WriteBatchRequest.messages` is non-empty and bounded by the shared
   `MAX_MESSAGE_ARRAY_ITEMS`; its HTTP JSON body is `{ "messages": [...] }`.
   The canonical send response carries `BatchSendOutcome` as the batch variant
   of the existing send response envelope.
2. `decode_request` recognizes both singleton and canonical array bodies.
   The peer listener performs the same post-decode authentication/provenance
   check as any peer singleton. Local and peer adapters then call the same
   `ApiRouter::route` path.
3. Route the array through the same validation, idempotence, and SQLite
   admission used by singleton writes. Validate the whole array before the
   durable transaction; the admission outcome is all-or-nothing for new rows.
   Exact duplicate IDs remain idempotent rather than an error.
4. After commit, emit the received-message hook once for each newly persisted
   message and never for an idempotent duplicate. Aggregate hook failures as
   warnings in the one successful array response; they never make admission or
   page confirmation fail.
5. The response identifies the exact accepted IDs in order. The sender
   confirms the entire submitted page only after that single response proves
   all IDs; any mismatch means no cursor confirmation.
6. Existing multi-item ACK-array rejection remains unchanged. The recovery
   grammar carries ordinary write messages only; it does not create an ACK
   array endpoint or a separate acknowledgement protocol.

## Authoritative deliverables

1. A default-off setting and read-only runtime view; disabled mode performs no
   pending-cursor query from the heartbeat path.
2. The canonical bounded array request/response codec in `atm-core`,
   normalize it into the existing write admission. Delete no AK.12 guard;
   amend it only with the two structurally constrained additions: one private
   heartbeat-driver entry point and one canonical array form. The amended
   guard must inspect the whole daemon-crate call graph and prove the batch
   sender is private, reachable only from that entry point, and cannot be
   called from the direct-send router or any listener. Identifier matching
   alone is insufficient.
3. The indexed oldest-page cursor read and exact whole-page confirmation.
4. The specified state machine attached to the existing serial heartbeat
   callback. The
   callback must make at most one outbound array request per invocation.
5. Structured event logs only: `HostName`, page size, ordered
   `AtmMessageId`s (or safe correlation IDs), state transition, and one
   `ReplayOutcomeCode`: `transport`, `deadline`, `protocol_validation`,
   `non_success_response`, `backlog_full`, or `tick_skipped_in_flight`. Do
   not add a health map, delivery observability service, scan loop, or
   dashboard.
6. The unit and integration evidence listed under Required validation.

## Paths to delete

None. AK.12 already deletes the retired resend and peer-array surfaces. AK.15
may add only the canonical batch form and the named heartbeat replay entry
point described in this document.

## Acceptance criteria

- Every state-table row is covered by an executable test and exhibits its
  listed action and next state.
- With `peer_heartbeat_replay = false`, a direct outage followed by recovery
  produces no replay, no cursor query in the heartbeat path, and no direct
  fast-path regression.
- With the option enabled, an observed unavailable→healthy transition sends
  exactly one bounded oldest page; a successful page confirms exactly that
  page; a failed page cannot retry until a later unavailable→healthy
  transition.
- The canonical `{ "messages": [...] }` body uses `decode_request` and the
  same `ApiRouter::route` and SQLite admission as singleton writes. It adds no
  peer endpoint, decoder, listener, route, or hook path.
- The receiver emits one hook per newly persisted array member, none for
  idempotent duplicates, and returns hook failures only as warnings.
- The direct singleton path remains one ordinary AK.11 write. It never sends
  an array, waits for heartbeat, or depends on replay state.
- The cap rejects overflow deterministically without expiry, eviction, or
  silent message loss; a test proves the typed backpressure result and event.
- The PR cites and tests the existing serial heartbeat-driver contract; an
  in-flight/deadline test proves no tick can submit a second page.

## Required validation

- `just lint`, `just test`, `just smoke localhost`, and `just smoke local-ip`.
- Default-off regression: direct outage then recovery produces no replay, as
  established by AK.13.
- Enabled state-table coverage: direct success; failed direct send; unhealthy
  heartbeat; first healthy recovery page; multi-page draining one page per
  heartbeat; failed page with no cursor advance; exact whole-page success;
  duplicate rows; hook-warning response; disabled toggle before a heartbeat.
- Driver-contract test names the existing heartbeat driver/callback and proves
  its callback is serialized. An injected in-flight request and deadline test
  proves a tick performs no second query/send and leaves the page unconfirmed.

## Explicit prohibitions

- No retry on every heartbeat: `AwaitingRecovery` may send only on an observed
  unavailable→healthy transition, and `Recovering` may send at most one new
  page per later healthy heartbeat. Only an unconfirmed page can remain
  pending.
- No replacement peer sender, endpoint, decoder, route, or persistence path.
- No unbounded backlog, expiry, eviction, payload cache, or implicit second
  queue. Backpressure is the one per-host cap above.
- No automatic activation, hidden compatibility enablement, or revival of
  deleted AK.5 mechanisms under another name.
- No payload mutation, sender-side received-message hook, or hook failure
  treated as a receive/replay failure.
- Do not touch `atm-peer-tls-interop` or `atm-storage/src/tls.rs`.

## Completion evidence and handoff

Commit the state-table tests, direct default-off regression, canonical codec,
and code-level evidence. AK.15 is complete only when all authoritative
deliverables and required validation are production-ready. It does **not**
close optional replay conformance; AK.17 solely owns the physical M4/M5/Windows
matrix and before/after direct-path benchmark on the merged AK.15 commit.
