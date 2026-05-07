# Phase R.14 — SQLite Root And Message-Thread Semantics

```yaml
plan_type: sprint_plan
phase: R
sprint: "R.14"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pR-s14-sqlite-threading
branch: feature/pR-s14-sqlite-threading
status: planned
estimated_scope: L
```

## Goal

Close the SQLite durable-state model by moving to the host-scoped database root and implementing the final message-thread / ack / ephemeral semantics that the daemon runtime must preserve.

## Scope Summary

This sprint finalizes the storage and workflow contract that later runtime sprints depend on: one host-scoped database at `~/.atm/db/mail.db`, linear successor chains, thread-level ack semantics, time-based ephemeral cleanup, and the SQLite test/error rules needed for a production implementation.

## Governing Requirements

- `REQ-RUSQLITE-STORE-001`
- `REQ-RUSQLITE-MIGRATION-001`
- `REQ-P-ACK-001`
- `REQ-P-THREAD-001` through `REQ-P-THREAD-005`

## Governing ADRs

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`
- `docs/adr/ADR-002-host-wide-daemon-singleton.md`

## Governing Boundaries

- `BOUNDARY-MailStore-Sqlite`
- `BOUNDARY-TaskStore-Sqlite`
- `BOUNDARY-RosterStore-Sqlite`
- `BOUNDARY-AtmProtocol`

## Prerequisites

- `R.13` lifecycle root is complete so the daemon has one authoritative runtime writer path.

## Hard Dependencies

- no heartbeat/status or replay sprint should assume final message-thread semantics before this sprint lands

## Non-Goals

- heartbeat/runtime status cache
- peer daemon transport
- watch/reconcile runtime

## Sub-Tasks

1. Host-scoped SQLite path and migration model
   Development work:
   - change the default durable SQLite path to `~/.atm/db/mail.db`
   - remove remaining per-team database assumptions from the runtime/store assembly path
   - define any migration/bootstrap work needed for existing per-team planning assumptions
   Required tests:
   - in-memory fixture coverage for normal store behavior
   - focused on-disk tests for reopen, migration, and filesystem-path behavior
   Required doc or boundary updates:
   - update `docs/atm-rusqlite/requirements.md`, `docs/atm-rusqlite/boundaries.md`, and `docs/project-plan.md` if assembly or path ownership changes

2. Linear successor-chain model
   Development work:
   - add `parent_id` / successor-chain support using a strict one-successor model
   - enforce "original sender only" updates
   - support exactly `add-details` and `supersede`
   Required tests:
   - schema/service tests proving one direct successor max
   - read-path tests showing terminal-node behavior for `supersede` and appended behavior for `add-details`
   Required doc or boundary updates:
   - update protocol/store DTO docs if successor metadata becomes explicit in boundary payloads

3. Ack and ephemeral semantics
   Development work:
   - keep `atm ack` as one visible reply with `requires_ack = false`
   - implement chain-level ack clearing/reopening semantics
   - make ephemeral retention time-based only with `stale_at`, no read-triggered deletion
   - hide read ephemeral messages from normal views while preserving `--view-all` until expiry
   Required tests:
   - ack-on-ack prevention test
   - chain-level ack reopen test when a successor arrives after prior ack
   - stale cleanup tests for expired ephemeral messages
   Required doc or boundary updates:
   - update ack and thread semantics across requirements/architecture if any field names or DTOs change

4. SQLite error and test-fixture closeout
   Development work:
   - replace the current one-code-fits-all `sqlite_error(...)` behavior with typed mappings
   - keep in-memory-first fixture rules explicit and prevent accidental writes to the production root
   Required tests:
   - error-mapping tests for constraint, busy-timeout, and filesystem/open failures
   - explicit fixture tests proving production root is never used by test setup
   Required doc or boundary updates:
   - align `docs/atm-rusqlite/requirements.md` and sprint docs with the final fixture model

## Split Recommendation

Prefer one sprint because the host-scoped DB root, message-thread rules, and ack semantics all change the same store contract. If the schema work proves too large, split into:
- `R.14.1` host-scoped DB + typed SQLite error mapping
- `R.14.2` successor-chain / ack / ephemeral workflow semantics

## Acceptance Criteria

- the production default durable path resolves to `~/.atm/db/mail.db`
- the daemon remains the only ATM-owned writer to that database; direct read-only consumers are documented and no hidden ATM write fallback bypass exists
- successor chains are strictly linear and only the original sender may append them
- one ack clears the chain through the current terminal node; a later successor on an ack-required thread makes the chain pending again
- ephemeral retention depends only on `stale_at` plus periodic cleanup, never on first-read deletion
- `sqlite_error(...)` no longer collapses all failures into `ATM_MESSAGE_VALIDATION_FAILED`

## Required Validation

- `cargo test -p atm-rusqlite`
- `cargo test --workspace`
- `just lint`

## Required Document Updates

- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/architecture.md`
- `docs/atm-rusqlite/requirements.md`
- `docs/project-plan.md`
- `docs/phase-R/issues.md`

## Risks And Watchouts

- leaving the database filename or root ambiguous will leak into every later sprint
- allowing branching successors will make ack and correction behavior much harder to reason about
- ephemeral cleanup must stay decoupled from read-path mutation to preserve direct-read compatibility

