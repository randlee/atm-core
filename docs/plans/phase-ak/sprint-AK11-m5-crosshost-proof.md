---
title: AK.11 Simple direct delivery and M5 cross-host evidence
status: ready_for_merge
branch: feature/pak-s11-m5-crosshost-proof
worktree: ../atm-core-worktrees/feature/pak-s11-m5-crosshost-proof
target: integrate/phase-ak
baseline: a412bf80
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.10 merged to integrate/phase-ak
merge_gate: AK.10 merge commit
parallel_safe: false
quality_findings: [AK5-CROSSHOST-PROOF-001]
---

# AK.11 — simple direct delivery and M5 cross-host evidence

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

This closes the direct-send correction and makes the sprint mergeable. The
physical M5 proof remains a required merge/QA evidence activity against this
exact merged baseline; it is not a reason to retain `messages[]`, a resend
cache, or a scheduler.

## Delivered implementation evidence

- `317a5d35` versions the workspace `1.4.0-beta-ak.11`.
- `88bca9d5` makes the post-persistence received-message hook the sole daemon
  emission point and carries hook errors as warnings.
- `a412bf80` completes the tightly bounded same-store receipt once, preserving
  the incoming immutable payload and suppressing a later duplicate hook.
- Local focused tests, `just test`, and the feature localhost smoke pass on
  the AK.11 daemon. The tracked test procedure includes a direct repeated-ID
  peer-HTTP check that records one hook emission only.

## Physical proof contract

After this branch merges, run M4→M5 and M5→M4 with the merged commit recorded
in a tracked, sanitized evidence bundle. Each direction proves:

1. `atm send` sends one ordinary singleton write to the canonical
   `/v1/atm/messages` route and the receiver reports the submitted message ID.
2. The receiver persists the write and emits one receiver-side nudge/hook;
   there is no sender-side nudge.
3. Reposting the exact immutable request with the same ID is idempotent and
   produces no second receiver hook.
4. An injected receiver-hook failure leaves the receive successful and is
   surfaced only as a warning.

Retain the proof under a non-ignored tracked path with the commit SHA, both
binary versions, host identities, sanitized peer configuration fingerprints,
timestamps, request/response IDs, and nudge observations. Do not record
credentials or key material.

## Explicit prohibitions

- No `PeerResendScheduler`, coordinator, timer, recovery page, or `messages[]`
  send path may be restored to obtain M5 evidence.
- No peer-only direct-write serializer or direct-route may be added.
- No sender-side received-message hook may be added.
- Do not touch `atm-peer-tls-interop` or `atm-storage/src/tls.rs`; they remain
  operator-directed quarantined working code.

## Required validation

- `just lint`, `just test`, `just smoke localhost`, and `just smoke local-ip`
  on the exact merged AK.11 baseline.
- The bidirectional physical proof above, with a reviewable tracked artifact.
- Review confirms that the peer adapter uses the canonical HTTP writer/reader,
  that the router has one direct sender call, and that the only hook emission
  follows durable receipt.

## Follow-up

AK.12 starts only after AK.11 merges. It removes the remaining deprecated
array/resend grammar and tombstones while collapsing peer ingress onto the
same decoder and canonical receive route. It must not recreate any retired
mechanism merely because an old test or boundary record expects it.
