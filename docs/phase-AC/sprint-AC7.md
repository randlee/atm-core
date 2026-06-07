# AC.7 SQL Server Readiness Proof

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.7
worktree: ../atm-core-worktrees/feature/pAC-s7-sqlserver-readiness-proof
branch: feature/pAC-s7-sqlserver-readiness-proof
status: planned
estimated_scope: medium
```

## Goal

Prove that the resulting `atm-storage` contract is backend-neutral, small
enough to audit directly, and ready for a future `atm-storage-sqlserver`
implementation without another architectural reset.

## Scope Summary

This sprint is doc- and proof-driven. It does not build SQL Server. It audits
the final post-cleanup contract, verifies that storage semantics no longer
assume Claude JSON or SQLite internals, and records the exact remaining work
for a future SQL Server backend.

Primary closure rule:
- `AC.7` is the primary closure sprint for the SQL Server readiness claim
- it must not become a backdoor code-rework sprint for wrapper deletion,
  capability decisions, or backend cutover work that earlier sprints owned

## Governing Sources

- `docs/plan-phase-AC.md`
- `docs/phase-AC/readiness.md`
- `docs/phase-AC/issues.md`
- `docs/phase-AC/type-ledger.md`
- `docs/phase-AC/crate-graph-migration-map.md`
- final post-`AC.6` state of `crates/atm-storage`

## Prerequisites

- `AC.6`

## Out Of Scope

- no SQL Server implementation yet
- no new storage semantics beyond the contract already closed by `AC.1`..`AC.6`
- no reopening of backend-specific type deletions or internalization decisions

## Deliverables

- `docs/phase-AC/sqlserver-readiness-proof.md` exists as the written SQL
  Server readiness proof against the actual final contract
- an explicit statement of which existing traits and canonical shared structs a
  future `atm-storage-sqlserver` backend must implement
- an explicit statement of which backend-specific concerns remain outside the
  shared contract and therefore do not block SQL Server
- a short remaining-work checklist for `atm-storage-sqlserver` that does not
  require another storage-architecture reset

## Execution Checklist

Implementation order for `AC.7`:

1. Audit the final `atm-storage` public surface and record the real trait and
   type count.
2. Verify the final contract remains semantic and CRUD-shaped:
   - no request/response-per-operation wrappers
   - no Claude-file concepts
   - no SQLite observability or assembly concepts
3. Recheck the crate graph and boundary ownership:
   - `atm-storage-* -> atm-storage`
   - no backend crate depends on `atm-core`
4. Write the SQL Server readiness proof from the actual code and docs, not from
   the original Phase AC intent.
5. Record any remaining SQL Server-specific follow-up as normal backend
   implementation work, not as architecture debt.

Proof this sprint must leave behind:

- SQL Server readiness is a demonstrated property of the simplified contract
  surface
- the future backend can be implemented by conforming to the existing contract
  rather than redesigning it
- any remaining work is backend implementation detail, not a sign that the
  shared contract still leaks Claude or SQLite assumptions

## Acceptance Criteria

- the repo documents SQL Server readiness as a consequence of the cleaned
  contract in `docs/phase-AC/sqlserver-readiness-proof.md`, not as a
  hypothetical wish
- no backend crate still depends on `atm-core`
- the final `atm-storage` surface is small enough to audit directly and to hand
  to a new backend implementation line
- any remaining SQL Server follow-up is framed as backend implementation scope,
  not unresolved architecture scope

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `cargo tree -p atm-storage`
- `cargo tree -p atm-storage-claude`
- `cargo tree -p atm-storage-rusqlite`
- `git diff --check`
- `rg -n "Request|Response" crates/atm-storage crates/atm-storage-claude crates/atm-storage-rusqlite -S`
- `rg -n "SqliteObservability|SqliteBoundaryAssembly|ClaudeCodeRosterMember|ClaudeCodeTeamRoster" crates/atm-storage docs/phase-AC -S`

## Required Document Updates

- `docs/phase-AC/sqlserver-readiness-proof.md`
- `docs/phase-AC/sprint-AC7.md`
- `docs/phase-AC/readiness.md`
- `docs/phase-AC/issues.md`
- `docs/project-plan.md`
- `docs/plan-phase-AC.md`

## Risks And Watchouts

- if this sprint discovers contract-shape defects that earlier sprints should
  have closed, the phase should not false-close as SQL Server-ready
- if the proof is written from plan intent rather than actual crate state, the
  readiness claim will be weak
- if backend-specific helper types are still visible in shared docs or public
  APIs, future SQL Server work will inherit the same drift under a new name
