# Sprint U.5 — SQLite Query Cutover And Query Simplification

```yaml
plan_type: sprint_plan
phase: U
sprint: "U.5"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pU-u5-sqlite-query-cutover
branch: feature/pU-u5-sqlite-query-cutover
status: completed
estimated_scope: L
```

## Goal

Move normal mailbox query selection for `atm list` and `atm read` fully onto
SQLite and simplify it to start from mutable state, only reading content rows
that are actually needed.

Completion note:
- completed; QA verified `atm list` and `atm read` run on the SQLite-backed mailbox path.

## Scope Summary

This sprint removes file-backed normal mailbox query selection for `atm list`
and `atm read`, makes those flows auditable as SQLite-backed paths, and aligns
the SQL diagrams with the real boundary methods and queries.

`atm ack` and `atm clear` are not part of this sprint's cutover. They remain
on their existing runtime path until a later sprint rewrites those command
families explicitly.

## Governing Requirements

- `REQ-CORE-MAILBOX-001`
- `REQ-CORE-WORKFLOW-001`
- `REQ-P-LIST-001`
- `REQ-P-READ-001`
- `REQ-P-ACK-001`
- `REQ-P-CLEAR-001`

## Governing ADRs

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`
- one-message-identity ADR from `U.2`

## Governing Boundaries

- `BOUNDARY-MailStore`
- `BOUNDARY-InboxIngress`
- `BOUNDARY-InboxExport`

## Prerequisites

- unified mutable message state from `U.4` is complete

## Hard Dependencies

- `U.4`

## Non-Goals

- roster/task redesign
- remote replay redesign
- CLI UX redesign outside the query-source change

## Sub-Tasks

Each sub-task must be concrete and reviewable.

Required shape for every sub-task:
- development work
- required tests
- required doc or boundary updates when the code changes architecture or ownership

1. Cut `atm list` onto SQLite
   Development work:
   - replace file-backed list selection with SQLite-backed mailbox metadata
   - start from mutable state and only project content needed for list rows
   Required tests:
   - list selection/count/filter regression tests
   Required doc or boundary updates:
   - update CLI and SQL query diagrams

2. Cut `atm read` onto SQLite
   Development work:
   - replace file-backed normal read selection with SQLite-backed selection
   - preserve thread/update semantics and the existing read-side ack visibility
     semantics from earlier sprints
   Required tests:
   - read selection tests, including thread/ephemeral/deleted cases
   Required doc or boundary updates:
   - update read-path architecture and diagrams

3. Remove forbidden JSON read paths
   Development work:
   - delete or isolate remaining normal-runtime JSON summary/source reads used
     by `atm list` and `atm read`
   - leave JSON reads only in the private watcher/import/export boundary
   Required tests:
   - targeted tests proving normal runtime behavior no longer depends on source
     files
   Required doc or boundary updates:
   - update requirements/architecture and query-report docs

## Split Recommendation

Do not split unless one command family proves blocked on an unresolved mutable
state decision. `atm list` and `atm read` should move together.

## Acceptance Criteria

- normal mailbox queries do not read Claude JSON directly
- `atm list` and `atm read` are auditable as SQLite-backed query paths
- `atm ack` and `atm clear` remain explicitly out of scope for `U.5`; no
  acceptance claim in this sprint depends on cutting them over
- query diagrams show the actual boundary methods, tables read, and error exits
  for every non-trivial mailbox query

## Required Validation

- `cargo test --workspace`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `cargo xwin check --workspace --tests --target x86_64-pc-windows-msvc`
- `just lint`
- `git diff --check`

## Required Document Updates

- `docs/project-plan.md`
- `docs/plan-phase-U.md`
- `docs/architecture.md`
- `docs/requirements.md`
- `docs/atm-rusqlite/query-diagrams.md` (update existing content to reflect SQLite query cutover)
- `docs/atm/flow-diagrams.md` (update existing content to reflect SQLite-backed list/read flows)
- regenerate diagram pages under `docs/atm/` and `docs/atm-rusqlite/` using `python3 docs/reports/generate_diagram_pages.py`

## Risks And Watchouts

- do not leave JSON summary/source reads hiding behind helper layers
- do not preserve query inefficiencies just because the old file path used them
- do not let the SQL diagrams drift from the actual boundary methods
