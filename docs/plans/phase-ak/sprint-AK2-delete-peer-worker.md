---
title: AK.2 Delete daemon peer worker
status: proposed
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.1
parallel_safe: false
---

# AK.2 — delete daemon peer worker

## Closure

Delete the daemon-owned host-qualified delivery worker before its replacement.
This sprint deletes queue/thread/reload machinery only. A host-qualified
origin record retains exactly one immutable outbound write and destination host
(`peerOutbound`) as durable message data; it is not a queue or worker state.
Host-qualified writes are not delivered until AK.4.

## Deliverables

1. Delete `peer_drain_coordinator.rs`, `PeerDeliveryCoordinator`, worker
   creation/join/shutdown, `PeerJob`, `PeerWork`, channels, per-message
   threads, and worker-only tests.
2. Delete `PostCommitWorkKey::PeerDelivery` and its router signal. Retain the
   existing local post-write nudge path.
3. Delete worker-only retry/recovery policy and observability. Do not replace
   them with renamed queues, task handles, threads, background scans, or an
   immediate SQLite reload.
4. Retain `peerOutbound` only as the immutable persisted write plus destination
   host. It starts no work, owns no retry state, and is the sole durable input
   for AK.3 canonicalization and AK.5's later timer backlog. Do not create a
   second outbox table or duplicate payload representation.
5. Update Phase AI status text, requirements, ADRs, architecture, boundaries,
   and project plan so none promise daemon worker delivery or replay.

## Required validation

- Source gate rejects deleted worker symbols in production code.
- Unit: host-qualified admission persists its origin ULID once and starts no
  queue, thread, reload, socket, DNS, or nudge.
- Unit: local write retains its ordinary local nudge.
- `just lint` and `just test` pass.

## Dependencies

Before every AK.2 development/fix round, merge AK.1 into AK.2. Start AK.2 as
soon as AK.1 is pushed; do not wait for QA. AK.2 must not merge to `develop`:
AK.4 restores delivery. Push AK.2, then start AK.3 with AK.2→AK.3 merge-forward.
`must_follow` is required because AK.2 applies AK.1's keep/discard decision;
it is not parallel-safe because both touch cross-host routing/provenance.
