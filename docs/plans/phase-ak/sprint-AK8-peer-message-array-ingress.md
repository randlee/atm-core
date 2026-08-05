---
title: AK.8 Atomic peer message-array ingress
status: complete
branch: feature/ak8-11-peer-message-array
worktree: ../atm-core-worktrees/feature/ak8-11-peer-message-array
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.5
merge_gate: AK.5
parallel_safe: false
---

# AK.8 — atomic peer message-array ingress

## Closure

Replace AK.4/AK.5's HTTP keep-alive sequence of independent writes with one
peer `POST /v1/atm/messages` body containing `messages[]`. The configured peer
listener continues to be the only receiver. It decodes either the ordinary
singleton write or an authenticated peer array, normalizes the items through
the same canonical inbound admission rules, persists the accepted array as one
unit, and returns one response only after that commit. It creates no second
listener, route, worker, nudge path, or cross-host-specific recipient path.

Historical context only: the AK.5 implementation uses per-frame peer writes
and per-item confirmation. AK.8 replaces only the receiver side with one
atomic `messages[]` ingress request. AK.9 exclusively owns sender migration,
whole-array `confirm_peer_delivery_batch`, and flattening
`PeerResendAggregate`; none is an AK.8 deliverable.

## Completion record

- Implemented and pushed as `34bff63e` on 2026-08-04.
- Passed `just lint`, `just test`, `just smoke localhost`, and `just smoke
  local-ip`.
- Physical M4↔M5 cross-host proof remains exclusively owned by AK.11 after
  the receiver and sender array work has merged; it is not duplicated here.

## Fixed contract

```rust
struct PeerMessageArray {
    messages: Vec<WriteRequest>,
}

fn admit_peer_messages_atomically(
    requests: Vec<WriteRequest>,
    source_host: HostName,
    runtime: &LocalServiceRuntime,
) -> Result<SendResponseEnvelope, AtmError>;

trait MessageStore {
    fn save_messages_atomically(&self, messages: &[Message]) -> Result<(), AtmError>;
}
```

`PeerMessageArray` is a wire body, not a second API route or a second receiver
service. The only route stays `POST /v1/atm/messages`. The peer listener adds
the configured source-host provenance to every item, performs the same
canonical validation and idempotency preparation that a singleton peer write
uses, and rejects the entire request before persistence when any item is
invalid, duplicated within the array, or exceeds existing request bounds.

After all items prepare successfully, the receiver uses the existing atomic
storage boundary to make the complete immutable array durable. A single
success response means exactly that commit completed; a validation, storage, or
response error is not success for any subset. Existing single-message CLI,
graft, and curl requests retain their existing request body and normalize to
the same canonical admission path.

The post-commit dispatcher registers ordinary local post-write effects only
after the durable commit. Each accepted item may produce its ordinary local
nudge, but a queue-full event, nudge failure, hook failure, or notification-log
failure is a warning after receive success and never changes the peer HTTP
response to failure. Nudge is not a delivery receipt and is never consulted by
the sender.

## Type and boundary inventory

| Item | AK.8 role |
| --- | --- |
| `PeerMessageArray` | New peer-only JSON request body carrying one non-empty bounded ordered `messages` array. It owns no I/O, retry, route, or lifecycle behavior. |
| Existing `POST /v1/atm/messages` and `PeerHttpListenerSet` | The sole receiver endpoint and lifecycle. AK.8 extends its decoder; it creates no batch URL, second listener, or alternate peer receiver. |
| Canonical write preparation/admission | Existing singleton rules, invoked for every normalized item before any array persistence. No local-vs-cross-host recipient branch is introduced. |
| `MessageStore::save_messages_atomically` | Existing sealed durable batch boundary used for a fully prepared peer array. It must not expose partial acceptance. |
| `SendResponseEnvelope` | Existing response family. AK.8 defines one peer-array success response after the full durable commit; it does not emit an item-by-item response stream. |
| Existing post-commit local-nudge queue and emitter | Retained best-effort post-commit effect. It is outside receipt success/failure and remains the canonical receive-side nudge path. |

No batch receiver trait, receiver worker, transaction coordinator, nudge
receipt, sender callback, alternate route, or per-item HTTP response framing is
authorized.

## Deliverables

1. Extend only the existing peer HTTP request decoder so one request body can
   express a bounded non-empty `messages[]` peer array. Preserve ordinary
   singleton CLI/graft/curl `WriteRequest` bodies and the existing route.
2. Normalize singleton and array items into the common canonical write
   preparation path. Apply peer provenance and all validation to every item
   before any array persistence. Reject duplicate origin ULIDs within the
   request deterministically.
3. Commit an accepted array through one existing atomic storage operation, then
   return one success response. An error at validation, preparation, or commit
   must leave no subset newly accepted by that request.
4. Signal ordinary post-commit local effects after the commit only. Explicitly
   prove that nudge/hook/notification failures are retained as warnings and do
   not turn an already committed peer receive into an error response.
5. Amend ADR-047 and the active direct-delivery requirements/boundaries to
   replace the old per-frame meaning of a batch with this one-request atomic
   receiver contract. AK.9 exclusively owns sender use and confirmation.

## Explicit prohibitions

- No new listener, URL, API service, receiver trait, worker, timer, task,
  channel, queue, or post-commit retry mechanism.
- No partial success response, per-item HTTP response, or persistence of a
  subset of a valid submitted array.
- No sender-side or receiver-side use of a nudge result as receive/delivery
  success.
- No change to local CLI/graft request routing or a local/cross-host receiver
  branch after normalization.

## Required validation

- Unit: singleton peer and one-item array normalize to the same prepared
  canonical write and response semantics.
- Unit: a multi-item array validates provenance, recipients, origin ULIDs, and
  duplicates for every item before persistence; one invalid item yields no new
  persisted member of the array.
- Storage integration: successful arrays are durable atomically, retain input
  order for post-commit effects, and preserve existing idempotent replay
  semantics.
- Integration: one `POST /v1/atm/messages` array produces one response only
  after all items commit and uses the same receiver/post-write route as curl,
  CLI, and graft ingress.
- Integration: injected nudge/hook/notification failure follows a successful
  peer receive response and is observable as a warning, never a receive error.
- Regression: existing singleton local and peer request framing remains
  compatible; malformed/oversized/empty arrays have no false commit or nudge.
- `just lint`, `just test`, `just smoke localhost`, `just smoke local-ip`, and
  isolated bidirectional `crosshost-curl-plain` evidence pass.

## Dependencies

Before every AK.8 development/fix round, merge AK.5 into AK.8. Start AK.8 as
soon as AK.5 is pushed; do not wait for QA. AK.8 PR completion waits for AK.5
merge. AK.9 may start only after AK.8 is pushed and merge-forwarded; it must
merge AK.8 before every development/fix round and before PR completion. AK.6
is already merged as `1edd1e94` and places no open-work dependency on AK.8.
