---
title: AK.11 Simple direct delivery correction
status: ready_for_merge
branch: feature/pak-s11-m5-crosshost-proof
worktree: ../atm-core-worktrees/feature/pak-s11-m5-crosshost-proof
target: integrate/phase-ak
baseline: a412bf80
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: corrected Phase-AK plan-doc PR merged and AK.10 code merged to integrate/phase-ak
merge_gate: corrected plan-doc PR plus accepted AK.10 code merge commit
parallel_safe: false
quality_findings: []
---

# AK.11 — simple direct delivery correction

## Closure

AK.11 corrects the direct send/receive path before the physical M5 proof. It
does not preserve a resend design for proof purposes.

At `a412bf80`, the direct path is:

1. A host-qualified locally persisted write makes one immediate ordinary
   `RequestEnvelope::Write` request through the shared `atm_core::api` HTTP
   writer and reader. The peer adapter owns only bounded resolve/connect and
   the authenticated provenance header; it owns no queue, timer, retry loop,
   resolver thread, connection pool, or scheduler.
2. Every HTTP write — local or peer — enters the canonical application router.
   A peer receipt is distinguished by authenticated ingress provenance, not by
   a peer-only direct-send body.
3. The sole received-message hook runs only after durable persistence. It is
   receiver-side only; a hook failure is returned as a warning and never makes
   the receive or the sender's delivery result fail.
4. A repeated same-ID peer receipt is ordinary idempotence and does not emit a
   second hook. The narrowly scoped localhost/same-host retained-origin case
   completes one receipt by replacing only the stored transport envelope with
   the exact received envelope; it does not alter the immutable payload and it
   emits exactly one receiver hook.

This closes the direct-send correction and makes the sprint mergeable. AK.11
does **not** close `AK5-CROSSHOST-PROOF-001`; AK.13 is the sole owner of the
physical M4/M5/Windows conformance and no-replay evidence. That separation
prevents a code-review PR from being held open by an independent physical lane.

## Delivered implementation evidence

- `317a5d35` versions the workspace `1.4.0-beta-ak.11`.
- `88bca9d5` makes the post-persistence received-message hook the sole daemon
  emission point and carries hook errors as warnings.
- `a412bf80` completes the tightly bounded same-store receipt once, preserving
  the incoming immutable payload and suppressing a later duplicate hook.
- `e7c5d9ea` makes the retired-resend boundary fail closed: its enforcement
  test rejects a revived scheduler/coordinator or second raw outbound entry.
- The reviewed `a412bf80..AK.11` diff contains no changes under
  `crates/atm-peer-tls-interop` or `crates/atm-storage/src/tls.rs`, preserving
  the explicit TLS quarantine.
- Local focused tests, `just test`, and the feature localhost smoke pass on
  the AK.11 daemon. The tracked test procedure includes a direct repeated-ID
  peer-HTTP check that records one hook emission only.

## Non-closing preliminary verification

AK.11's local focused tests, direct repeated-ID peer-HTTP check, and localhost
smoke establish code-review confidence only. They neither replace nor narrow
AK.13's physical proof contract and cannot close any cross-host proof finding.

## Explicit prohibitions

- No `PeerResendScheduler`, coordinator, timer, recovery page, or `messages[]`
  send path may be restored to obtain M5 evidence.
- No peer-only direct-write serializer or direct-route may be added.
- No sender-side received-message hook may be added.
- Do not touch `atm-peer-tls-interop` or `atm-storage/src/tls.rs`; they remain
  operator-directed quarantined working code.

## Known deferred, not blocking

AK.11 QA verifies F1/F2 acceptance only. The following pre-existing items are
explicitly out of AK.11 scope, already scheduled, and expected to remain at
AK.11 merge time; reviewers must not relitigate them as an AK.11 regression or
new finding:

- `decode_peer_write_request` still forks from `decode_request` — AK.12 F3.
- `PeerMessageArray`, `peer_array.rs`, `ApiRequest::PeerMessages`, and the
  `messages[]` decode branch remain — AK.12 F4.
- `boundaries/atm-daemon/peer-resend-scheduler.toml` and the counter-only
  `boundary_enforcement.rs::peer_resend_scheduler_direct_calls` remain —
  AK.12 F6.
- ADR-046/047 still describe `messages[]`/default-on cache as normal path —
  AK.14 F5.
- `REQ-CORE-TRANSPORT-002E` is not yet in `docs/requirements.md` — AK.14 F7.

These deferred items are ownership boundaries, not waivers: their named
follow-up sprint must close them before minimal Phase-AK closure.

## Acceptance criteria

- The direct host-qualified path emits exactly one ordinary singleton
  `RequestEnvelope::Write` through the shared writer/reader.
- The receiver-only hook follows durable persistence, reports failure only as
  a warning, and is not emitted by the origin path or a duplicate receipt.
- AK.11 retains compile-failing resend tombstones and a temporary fail-closed
  crate-wide boundary check until AK.12 replaces both in the same commit with
  its complete call-graph/visibility invariant. The temporary check rejects an
  active scheduler/coordinator definition or a second direct outbound entry;
  it is not prose-only protection.

## Required validation

- `just lint`, `just test`, `just smoke localhost`, and `just smoke local-ip`
  on the exact AK.11 candidate baseline.
- Review confirms that the peer adapter uses the canonical HTTP writer/reader,
  that the router has one direct sender call, and that the only hook emission
  follows durable receipt.
- The temporary AK.11 enforcement check has negative fixtures for a revived
  scheduler/coordinator and a second outbound entry. It must pass before the
  AK.11 code PR merges and remains active until AK.12 replaces it.

## Follow-up

AK.12 starts only after AK.11 merges. It removes the remaining deprecated
array/resend grammar and tombstones while collapsing peer ingress onto the
same decoder and canonical receive route. It must not recreate any retired
mechanism merely because an old test or boundary record expects it.
