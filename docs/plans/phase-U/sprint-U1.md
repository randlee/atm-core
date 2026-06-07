# Sprint U.1 — Delete `metadata.atm` Read-Path Dependence

```yaml
plan_type: sprint_plan
phase: U
sprint: "U.1"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pU-u1-metadata-atm-read-cleanup
branch: feature/pU-u1-metadata-atm-read-cleanup
status: completed
estimated_scope: M
```

## Goal

Remove ATM-owned read-path dependence on `metadata.atm.*` so Claude JSON is no
longer an ATM-owned truth surface for mailbox/runtime state.

Completion note:
- completed; QA verified the `metadata.atm` read-path cleanup on the merged U.1 branch.

## Scope Summary

This sprint removes ATM-owned machine-state reads from message JSON, redirects
required state to SQLite-owned surfaces, and deletes dead compatibility code
that only exists to preserve `metadata.atm` read behavior.

## Governing Requirements

- `REQ-CORE-MAILBOX-001`
- `REQ-CORE-WORKFLOW-001`
- `REQ-CORE-BOUNDARY-001`
- `REQ-P-SEND-001`
- `REQ-P-READ-001`
- `REQ-P-ACK-001`
- `REQ-P-CLEAR-001`

## Governing ADRs

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`
- `docs/adr/ADR-010-claude-jsonl-compatibility-envelope.md`

## Governing Boundaries

- `BOUNDARY-MailStore`
- `BOUNDARY-InboxIngress`
- `BOUNDARY-InboxExport`
- `BOUNDARY-ConfigIngress`

## Prerequisites

- the current Phase U decisions recorded in `docs/project-plan.md`
- the Phase U summary in `docs/plans/phase-U/plan-phase-U.md`

## Hard Dependencies

- none; this is the opening cleanup sprint for the Phase U line

## Non-Goals

- removing the task or roster store surfaces
- redesigning message identity in full
- changing CLI-visible behavior beyond deleting ATM-owned JSON state reliance

## Sub-Tasks

Each sub-task must be concrete and reviewable.

Required shape for every sub-task:
- development work
- required tests
- required doc or boundary updates when the code changes architecture or ownership

1. Inventory and classification of `metadata.atm` reads
   Development work:
   - identify every production-path read of `metadata.atm.*`
   - classify each read as:
     - required SQLite-owned state
     - compatibility-only output
     - dead code
   Required tests:
   - add one inventory-backed regression check or equivalent targeted test for
     every surviving approved read
   Required doc or boundary updates:
   - update `docs/architecture.md` and `docs/atm-message-schema.md` to state
     that the active implementation preserves zero surviving `metadata.atm`
     fields

2. Redirect or delete runtime reads
   Development work:
   - remove ATM-owned runtime/query behavior that reads `metadata.atm.*`
   - move any still-needed state lookup behind `MailStore` or approved
     boundary-owned projections
   Required tests:
   - `send`, `read`, `list`, and `ack` regression tests proving behavior
     survives without `metadata.atm` reads
   Required doc or boundary updates:
   - update relevant boundary docs if state ownership moves from JSON to
     store/query surfaces

3. Compatibility output hardening
   Development work:
   - remove the `metadata.atm` namespace from active compatibility output
   - delete dead helper code that writes unapproved ATM-owned metadata
   Required tests:
   - serialization tests proving remaining compatibility output is bounded and
     does not become a hidden read dependency again
   Required doc or boundary updates:
   - tighten `docs/requirements.md` and `docs/architecture.md`

## Split Recommendation

Do not split around compatibility output. The default outcome of this sprint
should be zero surviving `metadata.atm` fields, not deferral.

## Acceptance Criteria

- no normal ATM read/query/runtime path depends on `metadata.atm.*`
- no `metadata.atm` field survives as compatibility output — the entire
  namespace is removed; `docs/atm-message-schema.md` must be updated to
  reflect zero surviving `metadata.atm` fields
- dead compatibility helpers and tests that only preserved `metadata.atm`
  reads are removed

## Required Validation

- `cargo test --workspace`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `cargo xwin check --workspace --tests --target x86_64-pc-windows-msvc`
- `just lint`
- `git diff --check`

## Required Document Updates

- `docs/project-plan.md`
- `docs/plans/phase-U/plan-phase-U.md`
- `docs/architecture.md`
- `docs/requirements.md`
- `docs/atm-message-schema.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/requirements.md`

## Risks And Watchouts

- do not preserve a `metadata.atm` read “temporarily” without explicit
  approval
- do not let compatibility output silently become runtime truth again
- deleting reads may expose under-specified SQLite ownership gaps; those gaps
  should be moved to store state, not pushed back into JSON
