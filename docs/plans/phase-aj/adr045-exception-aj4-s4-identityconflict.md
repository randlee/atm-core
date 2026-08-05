---
title: "ADR-045 exception ruling: AJ.4 IdentityConflict-clearing gap deferred to AJ.5 deletion"
status: ruled
ruled_by: team-lead
ruled_at: 2026-08-04T16:40:00Z
---

# ADR-045 Exception Ruling

## Requirement

ADR-045 (`docs/adr/ADR-045-runtime-observation-attribution.md:54-57`) states:

> Identity change and malformed/suppressed observation are retained anomaly
> events, not lifecycle states. They never reject ingress, emit
> `IdentityConflict`, degrade readiness, alter cache eviction, or change
> routing/nudge/delivery behavior. ... An exception requires an explicit
> requirement, ADR, boundary record, and test.

`plan-phase-aj.md`'s binding Design Rules name `record_identity_conflict`,
`record_identity_conflict_for_test`, and the `AtmError::identity_conflict(...)`
return as explicitly scheduled for removal under AJ.5 (line 316-319), and
state AJ removes the `IdentityConflict` producer path outright (line 88-90).

## Finding and why it is NOT fixed on AJ.4

QA-1 on AJ.4 (`.triage/phase-AJ/findings/AJ4-RBQA-F001-IDENTITY-CONFLICT.ttl`)
found that ordinary `LocalCommand`-sourced traffic can silently clear an
existing `IdentityConflict` record via `merge_observation`, with no
liveness re-check -- a real gap in the pre-existing (pre-AJ.4)
`record_identity_conflict`/`IdentityConflict` mechanism that AJ.4's own
`touch_member` addition newly exercises more frequently.

arch-ctm's first fix attempt (commit `17935dbe`) added a
`may_clear_identity_conflict` gate to `merge_observation` to prevent this.
QA-2 (quality-mgr, verified directly against the code and both governing
documents above) found this fix itself violates ADR-045 and
`plan-phase-aj.md`: it adds new protected-lifecycle-state machinery to a
mechanism explicitly scheduled for deletion one sprint later, without the
"explicit requirement, ADR, boundary record, and test" exception ADR-045
itself requires to justify such an exception -- and the added gate is
independently unsound (does not re-verify liveness on same-cached-pid
heartbeat replay, reproducing the original defect via a different path).

## Ruling

Do not harden `record_identity_conflict`/`IdentityConflict` on AJ.4. This
finding (`AJ4-RBQA-F001-IDENTITY-CONFLICT`) is accepted as a known,
intentionally-deferred gap in legacy code one sprint from deletion, not a
regression AJ.4 must fix. AJ.4's fix commit `17935dbe`'s
`may_clear_identity_conflict` gate and its accompanying test must be
reverted; AJ.4 returns to its pre-fix behavior for this one finding only
(all other AJ.4 QA-1 findings, promoted to
`feature/pAJ-s10-runtime-observation-phase-closeout`, are unaffected by
this ruling).

## Boundary record

This document is that boundary record. No `boundaries/*.toml` change is
required since the exception is temporal (one sprint) and the mechanism
itself is already marked for deletion in `plan-phase-aj.md`.

## Test

The required "test" proving this gap is closed is AJ.5's own deletion of
the entire mechanism, already independently confirmed by 3 QA-1 reviewers
on `feature/pAJ-s5-heartbeat-session` (commit `db215834`): "All 'Paths To
Delete' items genuinely removed: `process_is_alive` guard,
`record_identity_conflict` call, `RuntimeStatusCache::record_identity_conflict`/
`_for_test`, the `IdentityConflict`/Degraded-readiness projection branch,
and all 3 named retired tests -- confirmed absent repo-wide by 3
independent reviewers." Once AJ.5 merges to `integrate/phase-aj`, this
finding class ceases to exist by design rather than by patch.
