# AS.3 — Prove canonical preflight from the exact sync worktree

```yaml
plan_type: sprint_plan
phase: AS
sprint: AS.3
worktree: sc-compose-publish-kit-migration
branch: plan/sc-compose-publish-kit-migration
status: proposed
estimated_scope: validation and evidence only
```

## Goal

Demonstrate that the exact synchronized worktree can complete a real,
non-publishing preflight with one resolved plan, toolchain, and validation
receipt.

## Scope Summary

Run validation and one GitHub preflight. Preserve evidence; do not publish or
modify shared files during this sprint.

## Governing Requirements

- All jobs use the same shared bootstrap and resolved toolchain.
- Preflight and release are bound by source, manifest, toolchain, and
  validation digests.
- Preflight is non-disclosing and fail-closed.

## Governing ADRs

- No new ADR.

## Governing Boundaries

- This sprint validates the canonical overlay; it does not repair it locally.

## Prerequisites

- AS.2 acceptance criteria are met and all required upstream shared gaps are
  accepted and synchronized unchanged.

## Hard Dependencies

- `AS.2`: `must_follow`; merge forward before dispatch.
- `AS.4`: `must_follow`; PyPI publication requires this evidence.
- `AS.5`: `must_follow`; no migration merge before this evidence.

## Non-Goals

- Production or TestPyPI publication.
- Tag creation, artifact rebuild, credential rotation, or retrying an
  unauthorized channel.

## Sub-Tasks

1. Run canonical shared-kit tests and ATM `just lint`/`just test` from the
   exact synchronized worktree.
2. Dispatch the canonical `release-preflight.yml` using the exact worktree’s
   source and manifest.
3. Retain resolved-plan, toolchain, validation, channel, and crates-state
   receipts; verify source, manifest, toolchain, and validation digests agree.
4. Verify every preflight/release job uses the shared bootstrap with exact
   resolved versions; reject any workflow-local tool installation drift.

## Split Recommendation

Keep this execution proof separate from publication authorization. AS.4 is a
different closure type and executes against immutable `main` artifacts.

## Acceptance Criteria

- Shared-kit tests, `just lint`, and `just test` pass.
- One real canonical preflight succeeds without publishing.
- All receipts agree on source, manifest, toolchain, and validation digests.
- No undeclared channel or local tool-install path executed.

## Required Validation

```bash
just lint
just test
PUBLISH_KIT_SOURCE=/Users/randlee/Documents/github/sc-publish
bash "$PUBLISH_KIT_SOURCE/docs/publish-kit/sync-overlay.sh" --dry-run "$PWD"
gh workflow run release-preflight.yml --ref "$(git rev-parse HEAD)"
```

## Required Document Updates

- Attach immutable workflow run IDs and receipt digests to this sprint.

## Risks And Watchouts

A green local check is insufficient; the GitHub preflight must prove the same
toolchain path used by publication.
