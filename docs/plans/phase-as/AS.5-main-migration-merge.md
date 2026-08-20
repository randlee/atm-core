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

Promote only the source-parity migration whose preflight and one-channel
publication evidence is accepted from `develop` to `main`, then retire legacy
release assets only where the canonical replacement is proven equivalent.

## Scope Summary

This sprint is a merge/retirement gate. It does not amend shared files or
broaden channel activation.

## Governing Requirements

- Exact upstream parity persists through merge.
- A legacy item may be removed only after its canonical replacement has a
  verified acceptance result.
- A human must separately authorize the release PR from `develop` to `main`;
  plan acceptance, CI, and QA are necessary evidence but are not production
  authorization.

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
3. Merge approved implementation sprints through `integrate/phase-AS` into
   `develop`, then open the release PR from `develop` to `main`.
4. Merge the release PR only after independent QA confirms parity/no
   release-design regression and a human explicitly authorizes production
   promotion.
5. Re-run the minimum canonical checks on merged `main`.

## Split Recommendation

Keep deletion review isolated from the next release to preserve rollback.

## Acceptance Criteria

- Merged `main` reports exact canonical parity.
- No direct feature/phase branch promotion to `main` occurred; the recorded
  release PR originated from `develop` and has explicit human authorization.
- Every removed legacy asset has a named canonical replacement and proof.
- No copied shared file differs from its accepted upstream source.
- QA approves no release-design regression.

## Required Validation

```bash
PUBLISH_KIT_SOURCE=/Users/randlee/Documents/github/sc-publish
python3 "$PUBLISH_KIT_SOURCE/plugins/sc-publish/install.py" \
  --input release/sc-publish-consumer-input.json --dry-run "$PWD"
just lint
just test
```

## Required Document Updates

- Update migration status and link parity/QA evidence.
- Record all deleted paths and their replacement proof.

## Risks And Watchouts

Do not remove legacy behavior merely because a similarly named generic
workflow exists; activation and evidence must be real.

## Legacy Retirement Review — 2026-08-20

Removed the bespoke `.github/workflows/hermes-atm-pypi-publish.yml` and its
private `scripts/prepare_hermes_atm_publish_artifacts.py` staging helper,
together with their tests. Their verified canonical replacement is the merged
manifest-driven `.github/workflows/pypi-publish.yml`: it verifies immutable
release assets with `verify-published-release`, selects only the manifest's
Python distributions, writes a source-bound receipt, and uses the declared
PyPI/TestPyPI channel configuration. AS.1–AS.4 accepted the canonical
consumer contract and PyPI preflight/publication path, so retaining the
bespoke `twine` workflow would leave two competing release paths.

Retained `scripts/validate_release.py`, `scripts/release_artifacts.py`,
`scripts/release_gate.sh`, and `scripts/verify_release_archive.py`. The
migration record still identifies installed-document archive verification and
the retained release-inventory schema as capabilities without a verified
canonical replacement. They remain local supporting validation only and are
not evidence for canonical publication eligibility.
