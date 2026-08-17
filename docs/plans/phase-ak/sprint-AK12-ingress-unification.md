---
title: AK.12 Shared ingress and retired resend-surface deletion
status: proposed
branch: feature/pak-s12-ingress-unification
worktree: ../atm-core-worktrees/feature/pak-s12-ingress-unification
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: corrected Phase-AK plan-doc PR merged, then AK.11 code merged to integrate/phase-ak
merge_gate: accepted AK.11 code merge after the corrected plan-doc PR
parallel_safe: false
quality_findings: [AK-MANDATE-002-INGRESS-FORK, AK-MANDATE-EXT-DRIFTED, AK-MANDATE-BOUNDARY-STALE]
---

# AK.12 — shared ingress and retired resend-surface deletion

## Closure

AK.11 made the production send and post-persistence receive behavior simple.
AK.12 removes the deprecated mechanisms still capable of pulling the design
back toward a peer-specific path. It closes F3, F4, and F6 together because
the old peer decoder is the only remaining consumer of the old array grammar.

The final shape is intentionally small:

```
local CLI / graft / configured peer HTTP
              │  same ordinary singleton write body
              ▼
        decode_request
              ▼
   thin authenticated-ingress tagging
              ▼
        one ApiRouter::route path
              ▼
     durable write → received-message hook
```

The peer listener may differ only in socket binding, extraction of the
authenticated `X-ATM-Peer-Source-Host` header, and attaching
`AuthenticatedIngress::Peer`. It must apply its writes-only authorization
*after* `decode_request`; it must not regain a decoder, body grammar, router,
or receive side effect of its own.

## Deliverables

1. Delete `decode_peer_write_request`. Both the local listener and peer
   listener call `decode_request`; the peer listener then verifies that the
   decoded request is an ordinary `ApiRequest::Write` before attaching peer
   provenance and routing it.
2. Delete every inactive array/resend surface in the same commit:
   - `crates/atm-core/src/send/peer_array.rs`, `PeerMessageArray`,
     `ApiRequest::PeerMessages`, and
     `write_peer_message_array_http_request`;
   - `messages[]` decoding, peer-array routing, and peer-array test fixtures;
   - `crates/atm-daemon/src/runtime_health/peer_resend_scheduler.rs` and
     `boundaries/atm-daemon/peer-resend-scheduler.toml` together;
   - resend configuration/storage accessors except the compatibility command
     that accepts `peer_resend_cache = false` and rejects `true`.
3. Replace `MessageStore::confirm_peer_delivery_batch(host, &[id])` with the
   single-purpose `confirm_peer_delivery(host, id)` direct-success operation.
   Delete the arbitrary-slice signature and every caller before this sprint
   merges. It is not a scheduler, recovery cursor, or array-delivery API.
   AK.15 may add a distinct whole-page confirmation operation only after the
   optional replay design is approved.
4. Replace count-only enforcement with a crate-wide structural invariant.
   Its complete daemon scope is `src/{lib,main}.rs`, `runtime_health.rs`,
   `runtime_health/peer_delivery_router.rs`,
   `runtime_health/post_commit_work.rs`, `peer_delivery_client.rs`,
   `peer_http_listener.rs`, `local_tcp_transport.rs`, and
   `local_ipc_transport/request_worker.rs`, plus every module declared from
   those roots. The guard must inspect module visibility and call edges, not
   merely identifier text, and prove all of the following:
   - `PostWriteRouter` is the only publicly reachable host-qualified outbound
     decision and has exactly one private direct-send edge to the shared
     writer; a renamed or relocated sender and any second caller fail;
   - no outbound branch emits the received-message hook;
   - fail if any production call/reference to
     `confirm_peer_delivery_batch` remains, or if the direct confirmation
     operation accepts more than one message ID;
   - fail if peer ingress calls a decoder other than `decode_request`, or
     routes through an API other than the canonical `ApiRouter::route`.
   Matching retired identifiers (`PeerResendScheduler`,
   `PeerDrainCoordinator`, `PeerDeliveryCoordinator`, `PeerMessageArray`,
   `send_peer_http_batch`, `decode_peer_write_request`, and `peer_array`) is
   supplementary tombstone evidence only. The guard is deliberately about
   prohibited structure, not a blanket ban on generic words such as `Worker`
   or `Manager` elsewhere in the daemon.
5. Unit-test the guard with representative fixtures proving that a resurrected
   scheduler/coordinator/array path, a renamed or relocated sender, and a
   second outbound caller each fail without placing dead production code in
   the tree.
6. Add parity tests using production serialization: the same ordinary
   singleton `WriteRequest` body must decode identically through the UDS/local
   adapter and the TCP peer adapter, apart from authenticated ingress
   provenance. Include the equivalent `curl` evidence for both sockets in the
   PR, but do not use curl as a substitute for exercising the production
   serializer.

## Explicit prohibitions

- No `messages[]` acceptance or serializer remains after this sprint.
- No sender retry, replay, queue, timer, scheduler, coordinator, worker,
  pool, per-endpoint health map, or peer-only sender is added. A future retry
  design requires a new operator-approved sprint; it cannot be revived by a
  test, compatibility shim, boundary record, or documentation reference.
- No duplicate persistence/hook path. The received-message hook remains one
  receiver-side post-persistence action and failure remains warning-only.
- Do not touch `atm-peer-tls-interop` or `atm-storage/src/tls.rs`.

## Acceptance criteria

- The whole-daemon structural invariant—not a router-file grep alone—rejects
  a second publicly reachable outbound peer sender, a renamed/relocated
  sender, and a second caller.
- The manifest and tombstoned resend module are deleted together in one
  commit, leaving no enforcement gap after AK.11's compile-failing tombstone.
- The only surviving direct confirmation API accepts exactly one existing
  `HostName` and one existing `AtmMessageId`.

## Required validation

- `just lint`, `just test`, `just smoke localhost`, and `just smoke local-ip`.
- A structural-guard test proves all three: a second outward-reachable sender
  or caller fails, a renamed/relocated retired sender fails, and a peer
  decoder other than `decode_request` fails.
- A direct local/peer parity test proves the same production singleton body
  reaches the same canonical route and persists once; repeated same-ID
  delivery is idempotent and does not issue another receiver hook.
- A direct-send confirmation test proves one exact message ID is retired and
  a source guard proves no pre-AK.15 multi-ID confirmation API remains.
- Report LOC delta. This is deletion-dominant; any substantial positive delta
  requires explicit review against the diagram above.

## Dependencies and handoff

Starts only after the corrected plan-doc PR and then AK.11's code merge. AK.13 follows AK.12 with physical direct-path and
no-replay proof; AK.14 then reconciles ADRs, requirements, package
documentation, and boundary records to that proven single-path baseline. The
former standalone resend-removal scope is absorbed by this sprint and must not
be dispatched separately.
