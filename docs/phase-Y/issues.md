# Phase Y Blocking Issues

## Purpose

Record the remaining `Phase Y` issues that block landing the line on
`develop`, separate those from non-blocking follow-up hardening, and map each
blocking item to the sprint that must close it.

This is a planning artifact on a worktree off `develop`. It does not imply
that `Phase Z` may begin. `Phase Z` stays blocked until every blocking item in
this ledger is closed and the `Phase Yd` readiness record says the line is
ready for `develop`.

## Review Source

Primary production-readiness review:

- verdict: `NOT READY`
- review scope: `integrate/phase-Y @ 75444082`
- report sent to `team-lead` as:
  - `7b7ef1d5-80dc-427b-95dd-d89dbd2efeeb`

Key findings from that review:

1. `delivery_execution.rs`: recovered Claude SQLite-failure path could
   partially deliver `message[1]` while `message[2]` failed, violating the
   identical logical-payload-set contract.
2. `delivery_execution.rs`: production send/ack notification delivery still
   bypassed the `NotificationSink` boundary through
   `maybe_run_post_send_hook(...)`.
3. `runtime_health.rs`: daemon retained-runtime factory did not wire
   `NotificationSink` on the live production path.
4. `notification_runtime.rs`: shutdown was bounded to `3s`, but not fully
   deterministic once synchronous persistence was stalled.
5. `runtime_health.rs`: health reporting did not directly model
   notification-worker liveness.

## Blocking Before Develop

These items block `Phase Y` from landing on `develop`.

1. Recovered Claude logical-message-set closure is not yet proven on the final
   accepted line.
   - issue class:
     - behavioral correctness
   - historical owner:
     - `Y.12`
   - closure requirement:
     - the recovered Claude SQLite-failure path either materializes the full
       logical message set or fails hard

2. Production notification execution still bypasses the owned notification
   boundary.
   - issue class:
     - boundary ownership
   - historical owner:
     - `Y.13`
   - closure requirement:
     - send/ack notification execution must route through
       `NotificationSink::deliver(...)`
     - no production-path direct `maybe_run_post_send_hook(...)` bypass may
       remain

3. Daemon retained-runtime composition must install the live
   `NotificationSink`.
   - status:
     - `CLOSED: Y.16`
   - issue class:
     - production composition
   - historical owner:
     - `Y.13`
   - closure requirement:
     - the live retained runtime used by the daemon must construct and install
       the daemon-owned `NotificationSink`
   - closure evidence:
     - `Y.16` moves retained-runtime installation ownership into
       `atm_daemon::composition::compose_runtime`
     - the production retained runtime is built through
       `build_production_runtime(...)`
     - named proof test:
       - `production_runtime_installs_daemon_notification_sink`

4. The final accepted `Phase Y` line must be lint-clean, test-clean, and
   phase-end-review clean on the candidate merge line.
   - issue class:
     - release gate / phase-end closure
   - evidence source:
     - post-review fix batches `PY-EOP-FIX-1` and `PY-EOP-FIX-R2`
   - closure requirement:
     - the accepted merge candidate must include the end-of-phase fixes and
       pass the required validation stack before the line is proposed for
       `develop`

5. Health reporting must expose notification-worker liveness through a thin
   owner-provided signal, not through compensating logic inside
   `runtime_health`.
   - issue class:
     - operational readiness
   - closure rule:
     - if this remains a `develop` blocker, it must close with a simple
       runtime-owned liveness signal that `runtime_health` projects directly
     - do not grow `runtime_health` into a logic-heavy recovery layer

## Non-Blocking Follow-Up

These items are explicitly not reasons to delay `Phase Y` landing on
`develop`.

1. Notification shutdown determinism beyond the bounded production contract.
   - the `3s` bounded-shutdown concern from the original review is not itself a
     `develop` blocker for this line
   - follow-up hardening may tighten it later, but it must not be used to
     reopen the `Phase Y` scope indefinitely

2. Broad lint-rule or docs-only reinterpretations of `Y.12` / `Y.13`.
   - they may be valid later work
   - they are not substitutes for the blocking runtime, boundary, composition,
     and readiness issues listed above

## Sprint Mapping

- `Y.14` closes the recovered Claude logical-message-set blocker.
- `Y.15` closes the production notification boundary bypass blocker.
- `Y.16` closes the retained-runtime composition blocker.
- `Y.17` closes the accepted merge-candidate blocker by proving the required
  phase-end fix line is present on the accepted candidate and the candidate is
  validation-clean.
- `Y.18` closes or explicitly reclassifies the final liveness/readiness
  blocker, leaves the named `develop`-gate record, and explicitly authorizes
  `Phase Z` to begin only if the line is actually ready.

## Phase Z Rule

`Phase Z` does not begin while any item in **Blocking Before Develop** remains
open.
