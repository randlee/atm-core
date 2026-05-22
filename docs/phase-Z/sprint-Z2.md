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

## Exact Targets

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
- `crates/atm-core/src/service_runtime.rs`
- `crates/atm-core/src/send/mod.rs`
- `crates/atm-core/src/list.rs`
- `crates/atm-core/src/read/mod.rs`
- `crates/atm-core/src/clear/mod.rs`
- `crates/atm-core/src/ack/mod.rs`
- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm-core/src/team_admin.rs`
- `crates/atm-core/src/team_admin/restore.rs`
- `crates/atm-core/src/delivery_policy.rs`
- `crates/atm-rusqlite/src/shared_db.rs`
- the approved fix branch on `integrate/phase-Z`

## Deliverables

- fixed branch containing only `Z.1` finding closure work
- updated `docs/phase-Z/smoke-findings-ledger.md` with fix/defer disposition
  for every carried `Z.1` finding
- updated `docs/phase-Z/readiness.md` with the accepted `Z.2` verdict and head
- one explicit ownership model in docs and code:
  - `config.json` is watcher-owned ingress only
  - canonical roster truth is exposed through immutable
    `ClaudeCodeTeamRoster`-named public projection/view
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
  - `config.json` must be watched by the filesystem watcher and the watcher
    ingest path must be the only allowed reader/importer of team roster state
  - the only approved public roster knowledge surface is an immutable
    `ClaudeCodeTeamRoster` projection/view derived from the canonical imported
    roster state
  - `Z.2` must define and implement the first-team bootstrap handoff that
    ingests a new team's watcher-observed config-owned roster into canonical
    SQLite roster truth before first communication on a clean host
  - do not reintroduce direct delivery-harness resolution from `config.json`
  - do not leave any retained command or team-admin path reading `config.json`
    for roster truth after this sprint closes
- close `Z1-F002` by fixing SQLite schema-init ordering for preexisting
  host-scoped databases whose `mail_messages` table still exposes
  `legacy_message_id` instead of `message_id`
  - the migration path must succeed before daemon IPC publish
  - the fix must preserve clean-start behavior for brand-new databases
  - the fix must not silently discard or rewrite preexisting mail rows outside
    the approved schema migration path

## Paths To Delete

`Z.2` must enumerate and remove every production `config.json` roster-truth
touch-point outside the watcher-owned ingress/import path. The current direct
touch-points to delete or narrow away from roster-truth use are:

- `crates/atm-core/src/send/mod.rs`
  - `validate_send_target(...)` reads `runtime.load_team_config(...)` for
    recipient membership checks before send
- `crates/atm-core/src/list.rs`
  - explicit-target membership validation reads `runtime.load_team_config(...)`
- `crates/atm-core/src/read/mod.rs`
  - explicit-target membership validation reads `runtime.load_team_config(...)`
- `crates/atm-core/src/clear/mod.rs`
  - explicit-target membership validation reads `runtime.load_team_config(...)`
- `crates/atm-core/src/ack/mod.rs`
  - ack and reply-team membership checks read `runtime.load_team_config(...)`
- `crates/atm-core/src/doctor/mod.rs`
  - team roster / baseline reporting reads `runtime.load_team_config(...)`
- `crates/atm-core/src/team_admin.rs`
  - retained `members`, `add-member`, and backup paths read or write
    `config.json` directly
- `crates/atm-core/src/team_admin/restore.rs`
  - restore reads current/backup `config.json` and writes updated config
- `crates/atm-core/src/service_runtime.rs`
  - local runtime exposes `load_team_config(...)` as a normal runtime helper
- `crates/atm-core/src/config/mod.rs`
  - `load_team_config(...)` remains the file parser but must stop being a
    general roster-truth utility reachable from normal runtime flows
- `crates/atm-core/src/boundary_support.rs`
- `crates/atm-core/src/direct_boundaries.rs`
- `crates/atm-daemon/src/boundary_adapters.rs`
- `crates/atm-daemon/src/direct_boundaries.rs`
  - `ConfigIngress::load_team_config(...)` may survive only if narrowed to the
    watcher-owned import path; it must not remain a general roster lookup path

If any listed touch-point survives the sprint, the sprint doc and closeout
record must explain why that survival does not violate the watcher-owned single
reader rule.

## Acceptance Criteria

- every `Z.1` finding is either fixed or explicitly deferred with `team-lead`
  approval recorded in `docs/phase-Z/smoke-findings-ledger.md`
- any deferred `Z.1` finding is resolved or explicitly waived by `team-lead`
  before `Z.3` may begin
- `Z1-F001` closeout proves that a clean host with no preexisting SQLite DB
  and a new team config can complete the first daemon-backed communication path
  without bypassing canonical SQLite roster ownership
- every production `config.json` roster-truth touch-point listed in `Paths To
  Delete` is either removed or narrowed to the watcher-owned ingest/import path
- the only approved public roster knowledge surface after the sprint is an
  immutable `ClaudeCodeTeamRoster` projection/view; retained commands must not
  parse or consult `config.json` for roster truth
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
- `Z.2` does not relax canonical SQLite roster ownership by allowing
  `config.json` to become general runtime truth
- `Z.2` does not introduce a second public mutable roster surface beside the
  immutable `ClaudeCodeTeamRoster` view

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
- `git diff --check`
