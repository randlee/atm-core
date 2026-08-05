---
title: AK.13 Direct cross-host conformance and no-replay proof
status: proposed
branch: feature/pak-s13-direct-crosshost-conformance
worktree: ../atm-core-worktrees/feature/pak-s13-direct-crosshost-conformance
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.12 merged to integrate/phase-ak
merge_gate: AK.12 merge commit
parallel_safe: false
quality_findings: [AK5-CROSSHOST-PROOF-001, AK-MANDATE-NO-REPLAY-PROOF]
---

# AK.13 — direct cross-host conformance and no-replay proof

## Closure

AK.13 proves the deliberately minimal baseline on physical peers after AK.12
has removed all deprecated resend/array mechanisms. It is an evidence sprint,
not a recovery implementation sprint. A separately approved AK.15 may later
add default-off heartbeat-triggered replay only after this no-replay baseline
has been accepted.

The design being proved is exactly this:

1. A host-qualified send persists locally and makes one immediate ordinary
   singleton write to the remote daemon's canonical `/v1/atm/messages` route.
2. The receiver uses the same `decode_request` and `ApiRouter::route` path as
   local ingress, persists once, and only then runs the received-message hook.
3. A failed direct attempt returns the typed persisted-but-unconfirmed result.
   It may leave the existing durable outbound marker for audit/explicit
   operator action, but it causes **no automatic retry, replay, scan, timer,
   queued delivery, or delivery on later network recovery**.
4. A duplicate exact request is receiver-side idempotence: informational,
   successful, and no second receiver hook. It is not sender-side replay.

## Physical conformance matrix

Run against the exact AK.12 merge SHA and signed daemon binary on the M4
machine, the M5 Mac, and the Windows peer. Record daemon/CLI versions, host
identities, sanitized configuration fingerprints, and timestamps in a tracked
evidence bundle. Never retain credentials or key material.

For every available direction in the matrix (M4↔M5 and M4↔Windows):

1. Send one host-qualified CLI message. Prove the receiver accepted the same
   message ID through the canonical route, persisted it once, and emitted one
   receiver-side hook. Prove the origin emitted no hook.
2. Post the exact same immutable write once more through the authenticated
   peer endpoint. Prove the response is idempotent and the receiver hook count
   remains one.
3. Inject one bounded network failure that prevents the remote request from
   completing. Prove the origin returns the typed unconfirmed result and the
   receiver has neither the message nor a hook during that attempt.
4. Restore connectivity and observe for at least two configured historical
   resend due windows (or 2× the prior documented timer interval, recorded in
   the evidence). Prove that no request arrives, no message is persisted, and
   no hook runs without a new explicit operator send. This is the no-replay
   acceptance condition.
5. Perform one new explicit send after restoration. It must be a new direct
   singleton request and must satisfy step 1; it must not flush or transform
   any earlier failed attempt into a batch.

If a platform is unavailable, record the unavailable lane as open; do not
manufacture a mock, localhost, or ignored-log substitute. M4↔M5 is mandatory
for closing `AK5-CROSSHOST-PROOF-001`; Windows coverage is mandatory before
phase-wide final QA closure.

## Deliverables

1. A non-ignored, sanitized evidence bundle under
   `artifacts/phase-ak/AK13-direct-crosshost-conformance/` containing a
   README, machine-readable manifest, normalized command/config transcript,
   result IDs, receiver hook observations, outage interval, and explicit
   no-replay observation.
2. A reproducible operator procedure in `docs/peer-pair-smoke.md` for the
   three proof lanes: direct delivery, duplicate idempotence, and outage then
   restoration without replay.
3. A review checklist mapping every observed result to the four closure
   statements above. The checklist must report a failed/blocked lane rather
   than translating it into a code redesign.

## Explicit prohibitions

- No resend cache, scheduler, timer, cursor advancement, batch body, or
  recovery sender may be added to make an outage test pass.
- No test-only alternate send/receive endpoint or body grammar.
- No claim that a manually reissued send proves automatic replay.
- No sender-side receiver hook, and no hook failure treated as receive
  failure.
- Do not touch `atm-peer-tls-interop` or `atm-storage/src/tls.rs`.

## Required validation

- `just lint`, `just test`, `just smoke localhost`, and `just smoke local-ip`
  on the exact AK.12 merged baseline before each physical lane.
- The AK.12 source guards must pass unchanged; the proof cannot weaken a gate
  or introduce a compatibility shim.
- Review checks the tracked artifact rather than relying on ignored smoke logs
  or a verbal report.

## Dependencies and handoff

Starts only after AK.12 merges. AK.14 follows this sprint and makes the
requirements, ADRs, boundaries, and QA checklist exactly match the proven
one-write/no-replay baseline. AK.15 is then the only authorized place to add
the optional heartbeat replay extension.
