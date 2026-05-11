# Sprint T.1 Integration Gate Patch

**Branch**: `integrate/phase-S`
**Base**: `integrate/phase-S @ bdac03c`
**PR target**: `develop`
**Status**: Planning

## Goal

Close the remaining open `INTG-*` findings already triaged on
`integrate/phase-S` so the Phase S merge gate is clean.

PR-target rationale:
- `develop` is intentional here
- `T.1` changes land on `integrate/phase-S`, are validated in the Phase S gate
  PR, and then flow forward through the `integrate/phase-S -> develop ->
  integrate/phase-T` sequence rather than opening a direct `integrate/phase-T`
  implementation path

## Deliverables

- close the open architecture findings:
  - `INTG-ARCH-001`
  - `INTG-ARCH-002`
- close the open documentation-sync findings:
  - `INTG-ATM-QA-001`
  - `INTG-ATM-QA-002`
  - `INTG-ATM-QA-003`
  - `INTG-ATM-QA-004`
  - `INTG-ATM-QA-005`
  - `INTG-ATM-QA-006`
- close the remaining test/runtime follow-up findings:
  - `INTG-FTQ-008`
  - `INTG-RBP-003`
  - `INTG-RBP-004`
  - `INTG-RBP-005`
  - `INTG-RSH-005`
  - `INTG-RSH-006`
  - `INTG-RSH-007`
  - `INTG-RSH-008`

## Key File Targets

- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/composition.rs`
- `crates/atm-daemon/src/local_ipc_transport.rs`
- `crates/atm-daemon/src/notification_runtime.rs`
- `crates/atm-daemon/src/peer_transport.rs`
- `docs/project-plan.md`
- `docs/plan-phase-S.md`
- `docs/phase-S/sprint-S14-runtime-plan.md`
- `docs/phase-S/sprint-S15-rusqlite-hardening.md`

## Acceptance Criteria

- every open `INTG-*` id listed above is either closed or explicitly re-triaged
  with a replacement record and rationale
- no `INTG-*` finding remains open because of missing ownership ambiguity
- doc-only findings are resolved on `integrate/phase-S`, not deferred into
  `integrate/phase-T`

## QA Pointers

- run a focused integration-gate recheck against the triage records rather than
  a new sprint-wide discovery pass
- require `req-qa` to verify doc-sync fixes against `docs/project-plan.md`,
  `docs/plan-phase-S.md`, and the affected sprint docs
- require `arch-qa` / hardening review for the runtime and spawn-failure fixes

## Notes

This sprint is intentionally narrow. New architectural work belongs in
`T.2`–`T.5`, not here.
