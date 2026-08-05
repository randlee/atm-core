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

## Authorized extension

When explicitly enabled, one small state machine may replay a bounded ordered
`messages[]` array after a failed direct delivery **only when the existing
daemon heartbeat reports that the peer link has recovered**. It is not a new
transport and it is not a general retry service.

The state machine has only the durable pending cursor and the current link
transition needed to answer: “did heartbeat change this peer from unavailable
to healthy?” On that transition it makes one bounded array send to the same
canonical `/v1/atm/messages` endpoint. It does not own a thread, worker,
coordinator, connection pool, resolver, background scan, or independent
timer. The existing heartbeat invokes it; no heartbeat transition means no
replay attempt.

## Non-negotiable constraints

1. Default is disabled. With the option disabled, AK.13's no-replay behavior
   remains byte-for-byte and behaviorally unchanged.
2. Immediate direct sends remain ordinary singleton writes through the shared
   HTTP writer/reader. The option never changes that fast path.
3. The replay array uses the same route, authentication/provenance handling,
   shared decode entry point, canonical `ApiRouter::route`, durable writer,
   and post-persistence receiver hook as singleton receipt. The only receiver
   extension is accepting one bounded `messages[]` form and normalizing it to
   the same canonical write admission for each item; it is not a second
   listener, decoder, router, or persistence path.
4. One whole-array accepted response advances the durable cursor once for the
   exact submitted set. Any connection/protocol/validation failure leaves the
   cursor unchanged. There is no partial advancement and no per-message
   ad-hoc resend loop.
5. A replayed same-ID message is normal receiver idempotence and does not
   produce another received-message hook. Sender-side hooks remain forbidden.
6. A receiver hook failure is a receiver warning after durable success, never
   a replay failure and never a reason to replay the request.

## Required design and proof before implementation

1. Write the explicit state transition table and durable cursor invariants;
   reject implementation if it needs more state than pending cursor plus
   heartbeat availability transition.
2. Extend the AK.12 guards rather than weaken them: permit only the named,
   sealed heartbeat replay entry point and one canonical array-normalization
   entry point. Keep all former coordinator/scheduler/worker patterns
   prohibited.
3. Unit- and integration-test default-off behavior, transition-triggered
   one-array replay, no replay without a healthy transition, whole-array
   atomic cursor advancement, idempotent duplicate admission, and warning-only
   hook failures.
4. Repeat the AK.13 M4/M5/Windows physical matrix with default off and on.
   Default-off must reproduce the no-replay outage result. Enabled mode must
   show exactly one replay after a recorded unhealthy→healthy heartbeat
   transition and no replay while unhealthy.

## Explicit prohibitions

- No retry on every heartbeat; only one recovery transition may trigger one
  bounded replay attempt.
- No replacement peer sender or peer-only endpoint/body codec.
- No automatic activation, hidden compatibility enablement, or revival of
  deleted AK.5 mechanisms under a different name.
- No relaxation of the receiver's single post-persistence hook path.

## Handoff

AK.15 is a new, explicitly approved enhancement after the minimal baseline
is proven. It must update the requirement checklist and ADRs in the same PR
so QA can distinguish default-off no-replay from enabled recovery replay.
