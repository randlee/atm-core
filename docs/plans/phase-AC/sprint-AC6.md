# AC.6 Cleanup And Deletion Closeout

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.6
worktree: ../atm-core-worktrees/feature/pAC-s6-cleanup-and-deletion-closeout
branch: feature/pAC-s6-cleanup-and-deletion-closeout
status: complete
estimated_scope: large
```

## Goal

Delete the obsolete storage/RPC scaffolding and close the residual wrapper and
backend-leakage surface against the final ledger.

## Scope Summary

This sprint is the closeout line for contract-surface deletion. It removes
obsolete request/response wrappers and backend-shaped leftovers after earlier
sprints have already made the contract and ownership decisions.

Production-ready commitment:
- every deliverable listed in this sprint is expected to land at a
  production-ready level for the deletion-closeout scope this sprint claims;
  documentation-only deletion or grep-only closure is not accepted

Primary closure rule:
- `AC.6` is primarily a verification and residual-deletion
  sprint
- it must not become the first place where shared contract ownership, Claude
  internalization, or SQLite capability decisions are actually made
- SQL Server readiness proof is out of scope here and moves to `AC.7`

## Governing Sources

- `docs/plans/phase-AC/plan-phase-AC.md`
- `docs/plans/phase-AC/readiness.md`
- `docs/plans/phase-AC/issues.md`
- the new `atm-storage` and backend crates

## Prerequisites

- `AC.4`
- `AC.5`

## Out Of Scope

- no SQL Server implementation yet
- no new storage semantics beyond what earlier AC sprints defined

## Deliverables

- [x] delete any surviving `SourceIngress*` / `ProjectionExport*` public wrapper
  families from:
  - `crates/`
  - `docs/plans/phase-AC/`
  - boundary TOMLs
- [x] delete any surviving public backend bundle helpers the ledger marks
  `delete-bundle`, including:
  - `RuntimeBundle`
  - `SqliteBoundaryAssembly`
- [x] confirm Claude-only projections remain internal to
  `crates/atm-storage-claude/`, including:
  - `ProjectionRosterMember`
  - `ProjectionRoster`
  - `SourceFileRecord`
- [x] confirm SQLite-only observability helpers remain internal to
  `crates/atm-storage-rusqlite/`, including:
  - `SqliteObservability`
  - `SqliteObservabilityEvent`
  - `SqliteObservabilityOutcome`
  - `NullSqliteObservability`
- [x] delete speculative task-store surfaces by default so they no longer
  appear as approved Phase `AC` contract; quarantine is allowed only if
  immediate deletion is blocked by unrelated stabilization work:
  - `TaskStore*Request` / `TaskStore*Response`
  - `TaskStore` / `TaskStoreDoctor` public contract assumptions
  - SQLite task-store ownership and related boundary claims
- [x] update `docs/plans/phase-AC/type-ledger.md` with final closure notes for
  each deletion family touched in this sprint
- [x] keep `AC.7` as the sole owner of SQL Server readiness proof language

## Ledger-Driven Deletion Sweep

`AC.6` is the explicit closure sprint against the `AC.0` type ledger.

## Execution Checklist

Implementation order for `AC.6`:

1. Run the full ledger as a deletion checklist, not just grep-driven cleanup.
2. Remove any wrapper family still standing from:
   - code
   - docs
   - boundary TOMLs
   - tests
3. Reconfirm that backend-only Claude and SQLite types are still internal after the deletion pass.
4. Update the ledger with final closure notes so later phases do not resurrect deleted seams by accident.
5. Hand the resulting contract surface to `AC.7` as the fixed basis for SQL Server readiness review.

Proof this sprint must leave behind:

- the old storage/RPC wrapper architecture is materially gone from the repo
- the remaining shared contract is small enough for direct manual audit
- the resulting contract surface is clean enough to serve as the fixed input to
  `AC.7`
- speculative task-store code is deleted outright unless the sprint records a
  concrete blocker that forces temporary quarantine instead
- any row still needing a first-time ownership decision in `AC.6` should be
  treated as a prior-sprint planning defect, not normal scope

## Acceptance Criteria

- the shared storage contract remains small enough to audit directly
- no remaining obsolete wrapper families survive only for compatibility with the old design
- the deletion sweep is checked against `docs/plans/phase-AC/type-ledger.md`, not only ad hoc grep patterns
- the explicit TaskStore deletion grep gate passes for trait and record types:
  `pub trait TaskStore`, `pub trait TaskStoreDoctor`, `TaskStoreTaskRecord`,
  `TaskStoreTaskMetadata`, and `TaskStoreDoctorReport`
- speculative task-store code is removed rather than normalized as a supported
  compatibility surface, except where a documented blocker forces temporary
  quarantine
- any quarantine claim is recorded under the affected
  `docs/plans/phase-AC/type-ledger.md` row with a `quarantine-reason`
  annotation that names the specific blocking branch, open issue, or failing
  test; quarantine survives `AC.6` only and must not roll into `AC.7+`
- `AC.6` does not reopen capability or ownership decisions that earlier sprints
  were required to close

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/lint_boundaries.py`
- `git diff --check`
- `rg -n "MailStoreQueryMailboxMetadataRequest|MailStoreQueryMailboxMetadataResponse|MailStoreQueryMailboxMetadataCountsRequest|MailStoreQueryMailboxMetadataCountsResponse|MailStoreBootstrapRequest|MailStoreBootstrapResponse|MailStoreTransactionRequest|MailStoreTransactionResponse|MailStoreUpsertMessageRequest|MailStoreUpsertMessageResponse|MailStoreLoadMessageRequest|MailStoreLoadMessageResponse|MailStoreLoadStoredMessageRequest|MailStoreLoadStoredMessageResponse|MailStoreRecordIngestReplayStateRequest|MailStoreRecordIngestReplayStateResponse|MailStoreLoadIngestReplayStateRequest|MailStoreLoadIngestReplayStateResponse|MailStoreHealthSnapshotRequest|MailStoreHealthSnapshotResponse|MailStoreRequest|MailStoreResponse|RosterStoreReplaceRosterRequest|RosterStoreReplaceRosterResponse|RosterStoreLoadRosterRequest|RosterStoreLoadRosterResponse|RosterStoreQueryMembershipRequest|RosterStoreQueryMembershipResponse|RosterStoreHealthSnapshotRequest|RosterStoreHealthSnapshotResponse|RosterStoreListTeamsRequest|RosterStoreListTeamsResponse|RosterStoreRequest|RosterStoreResponse" crates docs/atm-core docs/atm-runtime docs/requirements.md docs/project-plan.md -S`
- `rg -n "pub trait TaskStore|pub trait TaskStoreDoctor|TaskStoreTaskRecord|TaskStoreTaskMetadata|TaskStoreDoctorReport" crates -S`
- `rg -n "SourceIngressImportRequest|SourceIngressImportResponse|SourceIngressIdentityFingerprintRequest|SourceIngressIdentityFingerprintResponse|SourceIngressDiagnosticsRequest|SourceIngressDiagnosticsResponse|SourceIngressRequest|SourceIngressResponse|ProjectionExportRecordRequest|ProjectionExportRecordResponse|ProjectionExportReexportMessageRequest|ProjectionExportReexportMessageResponse|ProjectionExportAppendMessageSetRequest|ProjectionExportAppendMessageSetResponse|ProjectionExportRequest|ProjectionExportResponse|SqliteBoundaryAssembly|RuntimeBundle" crates docs/atm-core docs/atm-runtime docs/requirements.md docs/project-plan.md -S`

## Required Document Updates

- `docs/plans/phase-AC/sprint-AC6.md`
- `docs/plans/phase-AC/readiness.md`
- `docs/plans/phase-AC/issues.md`
- `docs/project-plan.md`
- `docs/plans/phase-AC/type-ledger.md`

## Risks And Watchouts

- if deletion is deferred, the old architecture will remain readable and therefore reusable by accident
- if docs keep stale wrapper names alive after code deletion, future work will quietly rebuild the old model
