# ADR-014: Runtime Health Projection And Liveness Signal Ownership

## Status

Accepted — amended by issue #1378

## Context

`Phase Y` closeout needs one final answer for notification-worker liveness on
the accepted `integrate/phase-Y` candidate line.

The production-readiness review identified a legitimate gap: runtime health did
not directly model notification-worker liveness. But the same review also
showed a design risk: `runtime_health` must not become a compensating recovery
layer that reconstructs subsystem behavior by polling queue internals, retry
state, or daemon-private control logic.

`Phase Yd` therefore needs an explicit rule for how health reporting consumes
subsystem liveness.

## Decision

`runtime_health` is a projection layer only.

It may report subsystem liveness only through thin owner-provided signals.
For notification-worker liveness, the owning runtime subsystem is
`NotificationRuntime`, and the accepted shape is a direct projection such as:

- `NotificationRuntime::worker_liveness() -> NotificationWorkerLiveness`

`runtime_health` may read that signal and include it in the health snapshot,
but it must not infer or reconstruct liveness by:

- replaying queue state
- recomputing retry semantics
- inspecting worker internals through ad hoc logic
- adding subsystem-specific recovery behavior inside the health layer

## Alternatives Considered

1. Make `runtime_health` infer liveness directly from queue and worker state.
- rejected because it couples health reporting to subsystem-private logic and
  invites compensating behavior in the wrong layer

2. Add a separate health-check service that polls the notification subsystem.
- rejected for `Phase Yd` because it expands scope and introduces a second
  ownership surface for the same readiness question

3. Reclassify notification-worker liveness as non-blocking.
- accepted only as an explicit documented fallback if the final accepted line
  cannot close the signal cleanly without violating the projection-only rule

## Consequences

- subsystem owners must expose thin liveness signals directly when liveness is
  part of a readiness gate
- `runtime_health` remains simple and projection-only
- future health additions must follow the same rule unless a later ADR changes
  the contract
- any attempt to add compensating recovery logic to `runtime_health` is a
  design regression and must be rejected in review

## Issue #1378 application: member-state projection

The same projection-only rule applies to agent/member state. The write-through
RAM master roster owns the one canonical `RuntimeMemberState` and its ephemeral
source, freshness, and revision metadata. `RuntimeHealth` may project scoped
member observations and aggregate counts from that owner, but it must not own a
second `HashMap<MemberKey, MemberRecord>`, merge Herdr observations itself, or
append out-of-roster observations to a team snapshot.

Doctor and CLI roster/status surfaces consume that same projection. A member
with no accepted observation renders `Unknown`/`Unobserved`; a failed refresh
preserves its last exact state while rendering `Unavailable` freshness. Neither
case invents `Dead` or `Offline`. This application does not move live state
into SQLite and does not add recovery behavior to `runtime_health`.
