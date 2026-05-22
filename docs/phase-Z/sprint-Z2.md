---
id: Z.2
title: Fix And Revalidate
status: planned
branch: feature/pZ-s2-fix-and-revalidate
worktree: ../atm-core-worktrees/feature/pZ-s2-fix-and-revalidate
target: integrate/phase-Z
---

# Sprint Z.2 — Fix And Revalidate

```yaml
plan_type: sprint_plan
phase: Z
sprint: Z.2
worktree: ../atm-core-worktrees/feature/pZ-s2-fix-and-revalidate
branch: feature/pZ-s2-fix-and-revalidate
status: planned
estimated_scope: medium
```

## Goal

Close the verified smoke findings from `Z.1` and re-run the executable
validation set on the fixed branch.

## Hard Dependencies

- `docs/plan-phase-Z.md`
- `docs/phase-Yd/readiness.md`
- `docs/phase-Z/readiness.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- accepted `Z.1` smoke closeout at `70f4fa7f`
- `Z.1` smoke results are the only finding source for this sprint

## Prerequisites

- `Z.1` complete
- the validated `Z.1` smoke findings ledger is frozen

## Exact Targets To Modify

- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/phase-Z/readiness.md`
- `docs/requirements.md`
- `docs/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-core/architecture.md`
- `docs/atm-daemon/boundaries.md`
- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/boundary_support.rs`
- `crates/atm-core/src/direct_boundaries.rs`
- `crates/atm-core/src/config/mod.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/delivery_policy.rs`
- `crates/atm-rusqlite/src/shared_db.rs`
- the approved fix branch on `integrate/phase-Z`

## Read-Only Context References

- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/list.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/clear/mod.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm-core/src/team_admin.rs`
- `crates/atm-core/src/team_admin/restore.rs`

## Deliverables

- fixed branch containing only `Z.1` finding closure work
- updated `docs/phase-Z/smoke-findings-ledger.md` with fix/defer disposition
  for every carried `Z.1` finding
- updated `docs/phase-Z/readiness.md` with the accepted `Z.2` verdict and head
- smoke revalidation result on the fixed branch

## Required Work

- fix only the findings promoted from `Z.1`
- only findings recorded in `docs/phase-Z/smoke-findings-ledger.md` are in
  scope; newly discovered issues found during `Z.2` must be recorded but not
  fixed in this sprint, using the `notes` field and `status: out_of_scope`
  marker in `docs/phase-Z/smoke-findings-ledger.md`
- keep the branch aligned with the approved `Phase Y` architecture and state
  machines
- rerun the frozen `docs/phase-Z/smoke-checklist.md` checklist after fixes
  land
- close `Z1-F001` by making first-team roster ingress succeed before the first
  daemon-backed send path depends on SQLite roster membership:
  - the accepted behavior remains: delivery harness resolution uses canonical
    SQLite roster state, not `config.json` directly
  - the approved pre-`Z.8` bootstrap mechanism is a daemon startup-time,
    one-shot ATM roster hydration helper named
    `hydrate_roster_from_team_config_if_empty(...)` that uses the existing
    `ConfigIngress` ingest seam only when the canonical ATM roster is empty
    and a Claude `config.json` is present
  - that startup-only hydration path is not watcher infrastructure and must not
    remain callable as a generic runtime membership lookup after bootstrap
  - when the canonical ATM roster already contains at least one member, the
    startup-only hydration helper must be a strict no-op: no
    `load_team_config(...)` call occurs and no roster write is emitted
  - Claude Code harness send flow must not use `config.json` as a pre-write
    membership block; after the SQLite write succeeds and the post-write
    notification path selects a Claude Code inbox target, ATM may compare the
    member against `config.json` and return the warning
    `'<member-name>' is not on claude code roster <atm-team>/config.json'`,
    but if the inbox exists the inbox write still occurs
  - the broader removal of retained `config.json` runtime reads, the
    single-reader watcher/import design, canonical member metadata ownership,
    and team backup/restore automation are follow-on scope reserved for
    `Z.5` through `Z.10`
- close `Z1-F002` by fixing SQLite schema-init ordering for preexisting
  host-scoped databases whose `mail_messages` table still exposes
  `legacy_message_id` instead of `message_id`
  - the migration path must succeed before daemon IPC publish
  - the fix must preserve clean-start behavior for brand-new databases
  - the fix must not silently discard or rewrite preexisting mail rows outside
    the approved schema migration path

## Sub-Tasks

1. Implement startup-only first-team bootstrap hydration for `Z1-F001`.
   Development work:
   - add the daemon startup-time one-shot ATM roster hydration path
   - gate it so it runs only when the canonical ATM roster is empty and a
     Claude `config.json` is present
   - prove the path is startup-only and not reusable as a generic runtime
     membership lookup
   Required tests:
   - prove a clean host with no preexisting SQLite DB hydrates ATM roster truth
     before the first daemon-backed communication depends on membership lookup
   - prove `hydrate_roster_from_team_config_if_empty(...)` is a strict no-op
     when the canonical ATM roster already contains at least one member:
     no `load_team_config(...)` call occurs and no roster write is emitted
   Required docs:
   - update `docs/atm-daemon/boundaries.md`
   - update `docs/phase-Z/smoke-findings-ledger.md`

2. Fix the preexisting SQLite schema-init ordering bug for `Z1-F002`.
   Development work:
   - fix daemon startup against copied/preexisting `mail.db` state that still
     exposes `legacy_message_id`
   Required tests:
   - prove the copied/preexisting DB starts without silently discarding mail
     rows
   Required docs:
   - update `docs/phase-Z/smoke-findings-ledger.md`

3. Re-run frozen smoke validation and close the records.
   Development work:
   - rerun the frozen smoke checklist after fixes land
   - stamp `Z.2` accepted head and verdict in `docs/phase-Z/readiness.md`
   Required tests:
   - `git diff --check`
   Required docs:
   - update `docs/phase-Z/readiness.md`

## Follow-On Inventory For Z.5+

The broader `config.json` ownership cleanup identified during `Z.1` / `Z.2`
analysis is intentionally deferred into `Z.5` through `Z.10`. Those later
sprints own the deletion inventory, immutable runtime roster view, boundary
narrowing, watcher-owned ingress, team-admin/restore redesign, and canonical
member-metadata migration.

Once `Z.2` closes, execution continues with `Z.5`, `Z.6`, `Z.7`, `Z.8`,
`Z.9`, and `Z.10` before `Z.3` canary begins.

## Acceptance Criteria

- every `Z.1` finding is either fixed or explicitly deferred with `team-lead`
  approval recorded in `docs/phase-Z/smoke-findings-ledger.md`
- any deferred `Z.1` finding is resolved or explicitly waived by `team-lead`
  before `Z.3` may begin
- `Z1-F001` closeout proves that a clean host with no preexisting SQLite DB
  and a new team config can complete the first daemon-backed communication path
  without bypassing canonical SQLite roster ownership
- the only permitted pre-`Z.8` roster ingress path is the startup-only
  one-shot ATM roster hydration call when the ATM roster is empty; no generic
  runtime config lookup is introduced
- when canonical ATM roster state is already non-empty, the startup-only
  `hydrate_roster_from_team_config_if_empty(...)` path is a verified no-op
  with no config load and no roster write side effects
- Claude Code harness send no longer blocks on `config.json` before the SQLite
  write; any config/roster mismatch is surfaced as a post-write warning while
  still writing an existing Claude inbox target
- `Z1-F002` closeout proves that a copied/preexisting host `mail.db` with the
  older `legacy_message_id` schema shape can start cleanly under the current
  daemon line
- the smoke checklist is rerun on the fixed branch
- all smoke-checklist rows linked to a `Z.1` finding marked fixed, not
  deferred, in `docs/phase-Z/smoke-findings-ledger.md` record a passing
  `z2_revalidation_verdict`; any such row that still fails is a blocking
  finding for this sprint
- all checklist rows that passed in `Z.1` still pass after the `Z.2` fixes;
  any new failure is a blocking finding for this sprint
- `docs/phase-Z/smoke-findings-ledger.md` records final per-finding
  disposition and revalidation outcome
- `docs/phase-Z/readiness.md` records the accepted `Z.2` head and verdict

## Non-Closure

- `Z.2` does not widen the smoke checklist
- `Z.2` does not begin canary or release-signoff work
- `Z.2` does not own the full `config.json` single-reader cleanup line; that
  work is reserved for `Z.5` through `Z.10`

## Production-Ready Expectation

Every listed `Z.2` deliverable is expected to land at a production-ready level
for the smoke-fix scope this sprint claims: the branch must be suitable for
promotion into canary, and the updated smoke findings ledger must fully close
the `Z.1` handoff without silent carry-forward.

## Required Validation

- `cargo build --release` or equivalent release build that refreshes the
  executable baseline under test
- rerun `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/phase-Z/readiness.md`
- `cargo test --workspace`
- `cargo test --workspace z2_startup_hydration_skips_non_empty_roster -- --nocapture`
  - expected: startup-only hydration helper exits without calling
    `load_team_config(...)` and without emitting roster writes when canonical
    ATM roster already contains at least one member
- `git diff --check`
- `rg -n "load_team_config\\(" crates/atm-core/src/send/mod.rs crates/atm-core/src/delivery_policy.rs`
  - expected: no production membership-gate matches
- `rg -n "load_team_config\\(" crates/atm-core/src/boundary_support.rs crates/atm-core/src/direct_boundaries.rs`
  - expected: at most one named startup-only roster-hydration match for `Z1-F001`
    closure; any surviving generic runtime lookup match is a failure
