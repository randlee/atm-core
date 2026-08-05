---
title: AK.11 Simple direct delivery correction
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

## Required validation

- `just lint`, `just test`, `just smoke localhost`, and `just smoke local-ip`
  on the exact AK.11 candidate baseline.
- Review confirms that the peer adapter uses the canonical HTTP writer/reader,
  that the router has one direct sender call, and that the only hook emission
  follows durable receipt.

## Follow-up

AK.12 starts only after AK.11 merges. It removes the remaining deprecated
array/resend grammar and tombstones while collapsing peer ingress onto the
same decoder and canonical receive route. It must not recreate any retired
mechanism merely because an old test or boundary record expects it.
