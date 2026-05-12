# Sprint U.2 — ADR: One Message Identity

```yaml
plan_type: sprint_plan
phase: U
sprint: "U.2"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pU-u2-one-message-identity
branch: feature/pU-u2-one-message-identity
status: planned
estimated_scope: M
```

## Goal

Adopt one logical ATM message identity, remove duplicated message-id storage,
and eliminate confusing `legacy_*` / `metadata.atm.messageId` machinery.

## Scope Summary

This sprint formalizes the one-message-identity rule at ADR level and applies
it through schema, runtime, and compatibility boundaries so ATM keeps one
logical id while Claude `message_id` remains only the boundary wire encoding.

## Governing Requirements

- `REQ-CORE-MAILBOX-001`
- `REQ-CORE-BOUNDARY-001`
- `REQ-P-SEND-001`
- `REQ-P-READ-001`
- `REQ-P-ACK-001`

## Governing ADRs

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`
- this sprint must add a new ADR for one message identity

## Governing Boundaries

- `BOUNDARY-MailStore`
- `BOUNDARY-InboxIngress`
- `BOUNDARY-InboxExport`

## Prerequisites

- `U.1` is complete so message-id cleanup does not preserve removed
  `metadata.atm` read assumptions

## Hard Dependencies

- `U.1`

## Non-Goals

- unified mutable-state redesign
- thread/update semantic hardening
- full query cutover

## Sub-Tasks

Each sub-task must be concrete and reviewable.

Required shape for every sub-task:
- development work
- required tests
- required doc or boundary updates when the code changes architecture or ownership

1. Write the one-message-identity ADR
   Development work:
   - define one logical ATM message identity
   - define Claude `message_id` as boundary wire encoding only
   - forbid duplicated stored identity fields for the same message
   Required tests:
   - add or update conversion tests for approved boundary encoding behavior
   Required doc or boundary updates:
   - add the ADR and align project/architecture/message-schema docs

2. Delete duplicated identity storage
   Development work:
   - remove `metadata.atm.messageId`
   - remove duplicated UUID/ULID persistence for the same logical identity
   - rename confusing `legacy_*` storage/type names to approved terms
   Required tests:
   - send/read/ack compatibility tests using the surviving identity flow
   Required doc or boundary updates:
   - update `MailStore` and message schema docs to the final naming

3. Boundary and query cleanup
   Development work:
   - redirect CLI/service/runtime paths to the surviving identity surface
   - remove dead compatibility plumbing
   Required tests:
   - targeted selection and addressing tests for read/ack/thread operations
   Required doc or boundary updates:
   - update query/diagram docs that still describe duplicate identity storage

## Split Recommendation

Do not split. The ADR and implementation cleanup should land in one sprint so
the code cannot keep drifting between old and new identity models.

## Acceptance Criteria

- `metadata.atm.messageId` is removed from the design and implementation line
- ATM persists one logical message identity only
- Claude `message_id` is treated as boundary encoding, not a second ATM-owned
  durable identity
- confusing `legacy_*` identity naming is removed or narrowed to one explicitly
  justified compatibility role

## Required Validation

- `cargo test --workspace`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `cargo xwin check --workspace --tests --target x86_64-pc-windows-msvc`
- `just lint`
- `git diff --check`

## Required Document Updates

- new ADR under `docs/adr/`
- `docs/project-plan.md`
- `docs/plan-phase-U.md`
- `docs/architecture.md`
- `docs/requirements.md`
- `docs/atm-message-schema.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/requirements.md`

## Risks And Watchouts

- do not preserve two identities under new names
- do not leave thread/read/ack code split across old and new id types
- do not silently keep compatibility helpers that reintroduce duplicated
  persistence
