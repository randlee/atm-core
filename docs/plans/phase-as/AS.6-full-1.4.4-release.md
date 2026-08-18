# AS.6 — Full canonical 1.4.4 release

```yaml
plan_type: sprint_plan
phase: AS
sprint: AS.6
worktree: main immutable release candidate
branch: main
status: proposed
estimated_scope: complete authorized manifest-driven release
```

## Goal

Execute the first complete ATM release through the merged canonical publish
kit, using one explicit 1.4.4 manifest and evidence chain.

## Scope Summary

This sprint includes version selection, readiness and final-main preflights,
explicitly authorized manifest channels, and public verification. It does not
introduce new channel behavior during release execution.

## Governing Requirements

- Version synchronization, manifest identity, shared toolchain, and all
  validations must be locked before publication.
- Readiness preflight occurs before main; final preflight occurs on exact
  immutable main.
- Partial registry state uses PF-3 actions, never an implicit version bump.

## Governing ADRs

- No new ADR.

## Governing Boundaries

- Only manifest-declared and explicitly authorized channels run.
- Any new requirement found during release becomes an upstream/next-sprint
  item; do not patch shared code under release pressure.

## Prerequisites

- AS.5 merged and verified on `main`.
- Accepted source parity, valid channel declarations, successful readiness
  preflight, and human production authorization.

## Hard Dependencies

- `AS.5`: `must_follow`; merge and validate before creating the candidate.

## Non-Goals

- Publishing an undeclared destination or changing shared code/workflows.

## Sub-Tasks

1. Select and lock synchronized 1.4.4 versions in the manifest.
2. Run readiness preflight, then final preflight on immutable main; compare all
   plan/toolchain/validation digests.
3. Run the explicit production channel set in manifest dependency order.
4. Verify public registry/releases and every declared Python wheel on 3.11,
   3.12, 3.13, and 3.14.
5. Store complete structured channel receipts and final release evidence.

## Split Recommendation

Do not fold post-release enhancement work into this sprint. Regressions become
new upstream or ATM work after the release is closed.

## Acceptance Criteria

- The version, source, manifest, toolchain, and validation evidence agree.
- All and only authorized manifest channels completed or have PF-3 accepted
  skip/block results.
- Public verification and Python 3.11–3.14 matrix pass.
- A complete receipt permits independent reconstruction of what was published.

## Required Validation

```bash
just lint
just test
gh workflow run release-preflight.yml --ref <immutable-main-commit>
```

## Required Document Updates

- Record release authorization, immutable commits, digests, run IDs, public
  URLs, partial-state decisions, and post-release verification.

## Risks And Watchouts

Never substitute a successful preflight from another source or toolchain for
the immutable-main receipt required by this release.
