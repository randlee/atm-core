---
title: AK.9 Unified peer batch send and atomic confirmation
status: complete
branch: feature/ak8-11-peer-message-array
worktree: ../atm-core-worktrees/feature/ak8-11-peer-message-array
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.8
merge_gate: AK.8
parallel_safe: false
---

# AK.9 — unified peer batch send and atomic confirmation

## Closure

Use AK.8's one-request peer array contract for both direct singleton delivery
and AK.5's recovered oldest-first page. One successful response atomically
retires exactly the submitted durable outbound markers. Replace the current
per-frame transport loop and per-message confirmation loop, then flatten
AK.5's one-field `PeerResendAggregate`; retain its bounded timer and state
semantics.

## Fixed contract

```rust
fn send_peer_http_batch(
    config: &PeerHttpRuntimeConfig,
    endpoint: &PeerEndpoint,
    writes: &[WriteRequest],
    deadline: RequestDeadline,
) -> Result<SendResponseEnvelope, AtmError>;

trait MessageStore {
    fn confirm_peer_delivery_batch(
        &self,
        canonical_host: &HostName,
        message_ids: &[AtmMessageId],
    ) -> Result<(), AtmError>;
}

struct PeerResendState {
    states: HashMap<PeerEndpoint, PeerConnectionState>,
    earliest_due: Option<Instant>,
}
```

The sender serializes the supplied ordered slice into exactly one AK.8 peer
array request, including the one-element direct-send case. It accepts only the
one whole-array success response. A timeout, malformed response, error
response, or broken connection leaves every submitted marker eligible for the
next attempt and returns the stable `AtmErrorCode::RemoteDeliveryUnconfirmed`
error code; it does not infer which remote messages might have arrived.

Only after that one success does `confirm_peer_delivery_batch` remove the
matching `peerOutbound` markers in one SQLite transaction. A failure while
retiring the marker set is surfaced as a local storage error and must leave the
entire set unchanged. This is the sender cursor: it moves once for the exact
array, never once per HTTP frame or per item.

AK.5's immediate `Connected` attempt passes a singleton slice. Its due
callback passes one bounded oldest-first page. Both call the same function and
share the same confirmation operation. The `Connected`/`Disconnected`/`Queued`
state machine, `earliest_due`, one due endpoint per loop turn, retry delay, and
no-I/O-under-lock property remain unchanged. `PeerResendAggregate` disappears:
the map stores `PeerConnectionState` directly.

The cache-disabled direct path is an equally required migration target. In
`DaemonRequestDispatcher::dispatch`, the host-qualified `else` arm reached
when `peer_resend_scheduler` is unset currently calls the direct frame sender.
AK.9 replaces that call with `send_peer_http_batch` and the one transactional
`confirm_peer_delivery_batch` operation. It must not route this fast path
through the scheduler, emit a local nudge, or preserve a compatibility sender.

## Type and boundary inventory

| Item | AK.9 role |
| --- | --- |
| `send_peer_http_batch` | Replacement sole outbound peer sender. It issues one HTTP request and reads one response for a supplied ordered slice; no keep-alive frame loop or response vector remains. |
| `PeerMessageArray` | AK.8's wire value. AK.9 uses it for both singleton and recovered-page sends; it does not create a second transport protocol. |
| `DaemonRequestDispatcher::dispatch` cache-disabled host-qualified `else` arm | Required direct-path migration target when `peer_resend_scheduler` is unset. It sends the singleton through `send_peer_http_batch` and invokes `confirm_peer_delivery_batch` only after the one whole-array response. |
| `MessageStore::confirm_peer_delivery_batch` | New sealed atomic durable marker-retirement operation. It verifies the canonical host and exact submitted message set in one transaction. |
| `PeerDeliveryConfirmation`, `MessageStore::confirm_peer_delivery` | Removed from active peer delivery after all direct and retry callers migrate to batch confirmation. |
| `PeerResendState::states` | AK.5 state map simplified to `HashMap<PeerEndpoint, PeerConnectionState>`. It retains `earliest_due` as the one serve-loop timer deadline. |
| `PeerResendScheduler` | Retained bounded state owner. It neither gains a coordinator, worker, queue, thread, pool, payload cache, nor a second cursor. |

## Deliverables

1. Replace `send_peer_http_frames` and its per-write request/response loop with
   `send_peer_http_batch`, which emits one AK.8 array request and accepts one
   whole-array response. Delete obsolete frame-reader/vector response behavior
   used only for peer delivery.
2. Replace singular peer-delivery confirmation with
   `confirm_peer_delivery_batch`, implemented as one storage transaction over
   the exact ordered input set and configured canonical host. Migrate immediate
   cache-disabled dispatch and enabled-scheduler due-page callers together; do
   not leave a compatibility sender path.
3. Prove direct singleton success, recovered-page success, response failure,
   and local marker-retirement failure preserve the all-or-nothing sender
   cursor contract.
4. Flatten `PeerResendAggregate` to a direct endpoint-to-state map. Preserve
   `earliest_due`, deterministic due selection, bounded page size, and mutex
   scope; do not add a coordinator or replace the timer with a worker.
5. Update ADR-046/ADR-047, requirements, daemon/storage boundaries, and
   smoke documentation to describe one request/one response delivery and one
   atomic outbound confirmation. Specifically amend
   `boundaries/atm-daemon/peer-resend-scheduler.toml` and the
   `DirectPeerResendAggregate` section of `docs/atm-daemon/boundaries.md`.

## Explicit prohibitions

- No HTTP keep-alive loop of independent peer writes, per-frame response
  vector, partial cursor advancement, or per-message confirmation transaction.
- No new sender trait, coordinator, worker, task, channel, queue, connection
  pool, retry policy, or second timer.
- No recovery decision based on a remote nudge, agent state, roster state, or
  session identity.

## Required validation

- Unit: singleton direct delivery and a recovered page each issue exactly one
  peer HTTP request and require exactly one matching whole-array response.
- Focused dispatch integration: with `peer_resend_scheduler` unset, a
  host-qualified write takes the cache-disabled `else` arm, issues exactly one
  singleton `messages[]` request through `send_peer_http_batch`, atomically
  confirms its marker only after success, returns
  `AtmErrorCode::RemoteDeliveryUnconfirmed` on transport failure with the
  marker retained, and emits no local sender-side nudge.
- Integration: a successful array response atomically removes every submitted
  `peerOutbound` marker; a failed/invalid/missing response removes none.
- Storage integration: a failure during batch marker retirement rolls back the
  full marker set; a mismatch of host or message set cannot retire unrelated
  durable records.
- Regression: replay after any transport error resends the complete retained
  array, while a success cannot create duplicate recovery delivery or nudge.
- Unit: flattening the aggregate preserves `Connected`, `Disconnected`, and
  `Queued` transitions, one earliest deadline, deterministic due ordering, and
  no network or SQLite work while holding the state mutex.
- Integration/smoke: enabled recovery performs one healthy transition batch
  with one receiver persistence result and ordinary non-fatal post-commit
  nudges; disabled cache makes no retry. Run bidirectionally for
  `crosshost-send`, `crosshost-ack`, and `crosshost-curl-plain`.
- `just lint`, `just test`, `just smoke localhost`, and `just smoke local-ip`
  pass.

## Dependencies

Before every AK.9 development/fix round, merge AK.8 into AK.9. Start AK.9 as
soon as AK.8 is pushed; do not wait for QA. AK.9 PR completion waits for AK.8
merge. AK.6 is already merged as `1edd1e94` and places no open-work dependency
on AK.9.
