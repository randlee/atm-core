# Phase Yc Issues Inventory

## Goal

Track the exact `Yc` production-readiness blockers and record what is
intentionally out of scope so later hardening or QA passes do not rewrite the
runtime sprint deliverables.

## In-Scope Blockers

1. Claude recovered degraded delivery still allows partial logical-message-set
   success on the SQLite-failure path.
   - owning sprint: `Y.12`
   - primary runtime seam:
     - `crates/atm-core/src/delivery_execution.rs`
   - required closeout:
     - the recovered Claude path either materializes the full logical message
       set or fails hard

2. Production notification execution still bypasses `NotificationSink`.
   - owning sprint: `Y.13`
   - primary runtime seams:
     - `crates/atm-core/src/delivery_execution.rs`
     - `crates/atm-core/src/service_runtime.rs`
     - `crates/atm-daemon/src/runtime_health.rs`
   - required closeout:
     - the production notification path executes through
       `NotificationSink::deliver(...)` and no direct
       `maybe_run_post_send_hook(...)` helper path remains on the live
       executor/runtime path

## Explicitly Out Of Scope For Y.12 And Y.13

- post-mortem lint recommendations in
  `integrate/phase-Y/.triage/phase-Yb/post-mortem.md`
- new lint-rule work
- new boundary-rule enforcement work unrelated to the two runtime blockers
- docs-only or lint-only reinterpretations of `Y.12` or `Y.13`

If any later task or review attempts to convert `Y.12` or `Y.13` into
lint-only, docs-only, or otherwise non-runtime implementation work, treat that
as a scope conflict and require user discussion before changing the plan.
