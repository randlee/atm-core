# AS.5 — Merge the verified migration to main

```yaml
plan_type: sprint_plan
phase: AS
sprint: AS.5
worktree: sc-compose-publish-kit-migration
branch: plan/sc-compose-publish-kit-migration
status: proposed
estimated_scope: source-parity merge and legacy retirement review
```

## Goal

Merge only the source-parity migration whose preflight and one-channel
publication evidence is accepted, then retire legacy release assets only where
the canonical replacement is proven equivalent.

## Scope Summary

This sprint is a merge/retirement gate. It does not amend shared files or
broaden channel activation.

## Governing Requirements

- Exact upstream parity persists through merge.
- A legacy item may be removed only after its canonical replacement has a
  verified acceptance result.

## Governing ADRs

- No new ADR.

## Governing Boundaries

- Shared-file corrections go upstream, synchronize, and re-run AS.3; they are
  never amended in this merge branch.

## Prerequisites

- AS.3 preflight proof and AS.4 PyPI proof are accepted.

## Hard Dependencies

- `AS.3`: `must_follow`.
- `AS.4`: `must_follow`.
- `AS.6`: `must_follow`; full release requires merged canonical path.

## Non-Goals

- A release operation or a new shared-kit feature.

## Sub-Tasks

1. Re-run canonical parity before merging.
2. Review every legacy workflow/helper proposed for deletion against the
   legacy-value coverage record; preserve it if no verified canonical
   replacement exists.
3. Merge the migration PR only after independent QA confirms parity and
   no regression in the release design.
4. Re-run the minimum canonical checks on merged `main`.

## Split Recommendation

Keep deletion review isolated from the next release to preserve rollback.

## Acceptance Criteria

- Merged `main` reports exact canonical parity.
- Every removed legacy asset has a named canonical replacement and proof.
- No copied shared file differs from its accepted upstream source.
- QA approves no release-design regression.

## Required Validation

```bash
PUBLISH_KIT_SOURCE=/Users/randlee/Documents/github/sc-publish
bash "$PUBLISH_KIT_SOURCE/docs/publish-kit/sync-overlay.sh" --dry-run "$PWD"
just lint
just test
```

## Required Document Updates

- Update migration status and link parity/QA evidence.
- Record all deleted paths and their replacement proof.

## Risks And Watchouts

Do not remove legacy behavior merely because a similarly named generic
workflow exists; activation and evidence must be real.
