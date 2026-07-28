# Sprint U.7 — Roster Simplification And Explicit Member Model

```yaml
plan_type: sprint_plan
phase: U
sprint: "U.7"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pU-u7-roster-simplification
branch: feature/pU-u7-roster-simplification
status: completed
estimated_scope: M
```

## Goal

Replace the duplicated roster truth model with one canonical member store and
make member lifecycle and harness behavior explicit in first-class fields.

Completion note:
- completed; QA verified the roster acceptance criteria on the merged U.7 branch.

## Scope Summary

This sprint removes the current whole-roster JSON plus per-member projection
duplication, defines the approved member fields, and moves Claude Code
`config.json` roster sync behind the private watcher/import boundary.

## Governing Requirements

- `REQ-P-TEAMS-001`
- `REQ-P-MEMBERS-001`
- `REQ-CORE-BOUNDARY-001`

## Governing ADRs

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`

## Governing Boundaries

- `BOUNDARY-RosterStore`
- `BOUNDARY-ConfigIngress`
- `BOUNDARY-WatchEventSource-File`

## Prerequisites

- `U.1` is complete so `config.json` truth does not leak into normal runtime
  paths

## Hard Dependencies

- `U.1`

## Dependency Notes

U.7 does not depend on U.2 (one-message-identity). Roster member identity
(`agent_name`, `team_name`) is independent of mailbox message identity
(`AtmMessageId`). Roster cleanup can proceed without waiting for U.2.

`recipient_pane_id` remains in scope for U.7 only as a runtime/routing
candidate. It is retained only if justified by the U.7 roster review.

`pid` is not part of the canonical roster-member model. It is transient
daemon-owned runtime state and must stay outside the U.7 canonical member row
and outside SQLite roster truth.

## Non-Goals

- task-store redesign
- mailbox query cutover
- daemon/runtime heartbeat redesign beyond the roster storage model

## Sub-Tasks

Each sub-task must be concrete and reviewable.

Required shape for every sub-task:
- development work
- required tests
- required doc or boundary updates when the code changes architecture or ownership

1. Define the canonical member model
   Development work:
   - make one roster truth with explicit member fields:
     - `team_name`
     - `agent_name`
     - `member_kind`
     - `harness`
     - `agent_type`
     - `model`
     - `metadata_json`
     - optional `recipient_pane_id` only if justified by the U.7 review
   Required tests:
   - schema and roster-store tests for the approved member model
   Required doc or boundary updates:
   - update roster architecture/requirements docs

2. Remove duplicated roster storage
   Development work:
   - remove the whole-roster JSON plus per-member projection duplication
   - keep one canonical roster store only
   Required tests:
   - roster load/query/update tests against the final one-store design
   Required doc or boundary updates:
   - update SQL diagrams if roster tables are documented there later

3. Config sync behind watcher/import
   Development work:
   - sync Claude Code `config.json` roster changes into the canonical roster
     store through the private watcher/import boundary
   - prevent normal runtime paths from reading `config.json` directly for
     roster truth
   Required tests:
   - ingest/sync tests proving `config.json` changes flow through the approved
     boundary
   Required doc or boundary updates:
   - update `ConfigIngress`, watcher, and roster ownership docs

4. Member lifecycle and harness behavior
   Development work:
   - implement explicit `member_kind` lifecycle semantics for permanent vs
     ephemeral members
   - make `harness` a behavioral enum with the approved values:
     - `claude-code`
     - `codex-cli`
     - `gemini-cli`
     - `opencode`
     - `hermes`
     - `python-graft`
   Required tests:
   - roster lifecycle tests for ephemeral-member removal consequences
   - harness serialization/validation tests
   Required doc or boundary updates:
   - update project/architecture docs with the approved enum set

## Split Recommendation

Only split if the final roster SQL shape is blocked on a product decision
outside the approved member field set. Otherwise land the truth-model cleanup
and `config.json` sync rules together.

## Acceptance Criteria

- ATM has one canonical roster truth
- `member_kind` is explicit and supports distinct permanent vs ephemeral
  lifecycle logic
- `harness` is a first-class behavioral enum with the approved initial set
- `agent_type` and `model` remain plain strings
- `metadata_json` is the only generic extension bucket
- `recipient_pane_id` remains an optional canonical member field because
  authoritative pane routing may already be known at Claude-code roster ingest
- Claude Code `config.json` roster changes are ingested through the private
  watcher/import boundary rather than treated as general runtime truth

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
- `docs/atm-core/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-rusqlite/architecture.md` (update roster schema ownership/shape)
- `docs/atm-rusqlite/requirements.md` (update clauses for roster table changes)

## Risks And Watchouts

- do not keep duplicate roster truth “for convenience”
- do not leave `config.json` as a hidden runtime truth path
- do not make `harness` a free-form string if behavior depends on it
