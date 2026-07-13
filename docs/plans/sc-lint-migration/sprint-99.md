---
id: SCLINT.99
title: Final Implementation Review And Upstream Product Reports
status: planned
branch: TBD - requires new phase identifier and human sign-off
worktree: TBD - execution worktree not assigned
target: develop
---

# Sprint 99 — Final Implementation Review And Upstream Product Reports

## Goal

After all implementation sprints close, perform one complete review of the
updated migration result and produce the final reports that drive upstream
sc-lint work and remaining ATM cleanup.

## Hard Dependencies

- `docs/plans/sc-lint-migration/sprint-13.md`
- `docs/plans/sc-lint-migration/gap-register.md`

## Exact Targets

- `docs/plans/sc-lint-migration/sprint-99.md`
- `docs/plans/sc-lint-migration/reports/sc-lint-gap-issues.md`
- `docs/plans/sc-lint-migration/reports/adapter-removal-audit.md`
- `docs/plans/sc-lint-migration/reports/sc-lint-product-improvements.md`
- `docs/plans/sc-lint-migration/gap-register.md`

## Deliverables

- one complete post-implementation review of the adopted `sc-lint` surfaces is
  performed
- `docs/plans/sc-lint-migration/reports/sc-lint-gap-issues.md` lists the gaps
  that should become GitHub issues or feature requests on the sc-lint repo
- `docs/plans/sc-lint-migration/reports/adapter-removal-audit.md` audits every
  remaining ATM-local adapter or workaround and states whether it should be
  deleted now, queued for deletion, or moved upstream
- `docs/plans/sc-lint-migration/reports/sc-lint-product-improvements.md`
  records improvements that would make the long-term sc-lint product better
  than the historical ATM-local implementation

## Acceptance Criteria

- no surviving adapter is left without an explicit keep/delete/upstream
  disposition
- no product gap discovered during implementation is omitted from the final
  gap-issues report
- the final review distinguishes:
  - missing sc-lint capability
  - ATM-local cleanup still owed
  - broader product improvement that is not a blocking migration gap

## Paths To Delete

- none in this sprint doc; this sprint reports residual deletion obligations

## Required Validation

- `rg -n "GitHub issue|feature request|adapter|workaround|delete|upstream|improvement" docs/plans/sc-lint-migration/reports/sc-lint-gap-issues.md docs/plans/sc-lint-migration/reports/adapter-removal-audit.md docs/plans/sc-lint-migration/reports/sc-lint-product-improvements.md`
- `git diff --check`
