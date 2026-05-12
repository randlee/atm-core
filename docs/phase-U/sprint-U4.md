# Sprint U.4 — Unified Mutable Message State

```yaml
plan_type: sprint_plan
phase: U
sprint: "U.4"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pU-u4-unified-message-state
branch: feature/pU-u4-unified-message-state
status: planned
estimated_scope: L
```

## Goal

Replace the split `mail_visibility_states` / `ack_state` design with one
canonical mutable message-state owner.

## Scope Summary

This sprint redraws the SQLite mailbox state model so content rows stay in
`mail_messages` while all mutable mailbox/runtime state moves into one unified
message-state surface. It also renames `stale_at` to `expires_at`.

## Governing Requirements

- `REQ-CORE-MAILBOX-001`
- `REQ-CORE-WORKFLOW-001`
- `REQ-P-THREAD-004`
- `REQ-P-THREAD-005`
- `REQ-P-CLEAR-001`
- `REQ-P-WORKFLOW-001`

## Governing ADRs

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`
- one-message-identity ADR from `U.2`

## Governing Boundaries

- `BOUNDARY-MailStore`

## Prerequisites

- `U.1` through `U.3` are complete

## Hard Dependencies

- `U.1`
- `U.2`
- `U.3`

## Non-Goals

- full query cutover
- roster/task redesign
- remote replay/ingest replay redesign

## Sub-Tasks

Each sub-task must be concrete and reviewable.

Required shape for every sub-task:
- development work
- required tests
- required doc or boundary updates when the code changes architecture or ownership

1. Define the unified state shape
   Development work:
   - design one mutable per-message state owner
   - include approved mutable fields such as:
     - read state
     - ack state
     - `expires_at`
     - delete/hide state
   Required tests:
   - schema/serialization tests for the unified state shape
   Required doc or boundary updates:
   - update store/architecture docs to remove the split-state model

2. Remove split ownership
   Development work:
   - remove the split between visibility JSON and separate ack columns
   - move `stale_at` out of `mail_messages`
   - rename it to `expires_at`
   Required tests:
   - store write/load tests proving the unified state is authoritative
   Required doc or boundary updates:
   - update SQL diagrams and store boundary docs

3. Delete-state and expiration semantics
   Development work:
   - keep delete/close as state mutation only
   - keep hard deletion cleanup-only
   - ensure expiration and deletion stay out of normal current-message queries
   Required tests:
   - read/list filtering tests for expired/deleted rows
   - admin-only visibility tests for deleted rows
   Required doc or boundary updates:
   - update requirements and architecture wording for expiration/deletion

## Split Recommendation

Only split if the final approved mutable-state field set is still unresolved.
If the field set is approved, land the schema and code cleanup together.

## Acceptance Criteria

- ATM no longer splits mutable message state across `mail_visibility_states`
  and `ack_state`
- `expires_at` is the canonical expiration field name
- delete/close remain state mutations only
- deleted rows remain hidden from normal queries and are admin-only

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
- `docs/atm-rusqlite/architecture.md`
- `docs/atm-rusqlite/requirements.md`
- `docs/atm-rusqlite/query-diagrams.md`

## Risks And Watchouts

- do not keep duplicate mutable state under new names
- do not leave `stale_at` on content rows while also introducing `expires_at`
- do not preserve JSON blobs where explicit mutable fields are now approved
