# AS.1 — Freeze the canonical publish-kit overlay

```yaml
plan_type: sprint_plan
phase: AS
sprint: AS.1
worktree: sc-compose-publish-kit-migration
branch: plan/sc-compose-publish-kit-migration
status: proposed
estimated_scope: documentation and upstream-contract closure
```

## Goal

Establish one reviewable, byte-exact adoption contract for the `sc-publish`
overlay before any ATM consumer activation.

## Scope Summary

The source SHA, owned paths, reasons for each path, and parity proof are the
deliverables. This sprint makes no release, workflow, action, helper, or
manifest behavior change in ATM.

## Governing Requirements

- `sc-publish/docs/publish-kit-requirements.md` is normative.
- The canonical sync script is the only copier and parity checker.
- No copied path may be locally edited.

## Governing ADRs

- No new ADR. This sprint preserves the existing repository ownership
  boundary rather than changing an architecture decision.

## Governing Boundaries

- Shared paths: upstream-owned and synchronized unchanged.
- ATM paths: manifest data and namespaced extension data only.

## Prerequisites

- Accepted `sc-publish` source SHA is reachable.
- Canonical `sync-overlay.sh --dry-run` is available from that source.

## Hard Dependencies

- `AS.2`: `must_follow`; it may only justify consumer data after AS.1 records
  the shared ownership contract.

## Non-Goals

- Editing shared overlay files in ATM.
- Activating publication channels.
- Repairing upstream shared behavior locally.

## Sub-Tasks

1. Record the accepted source SHA and execute the canonical sync tool twice:
   once to synchronize and once in `--dry-run` mode to prove no remaining
   byte difference.
2. Retain the 31-path audit in the supporting migration design as the
   complete decision record. The overlay is atomic: no individual path is
   selected, omitted, or patched locally.
3. File upstream requirements for every shared closure gap discovered by the
   audit: relative-dependency closure, installed-archive-member validation,
   resolved release-plan evidence, `extra_validations`, and Cargo-derived
   dynamic PEP 621 versions.
4. Require an upstream closure test that fails whenever a copied shared file
   references an unvendored relative dependency not declared as a generic
   consumer interface.

## Split Recommendation

Keep AS.1 isolated: it resolves ownership and evidence contracts only. Any
consumer manifest work belongs to AS.2.

## Acceptance Criteria

- One accepted `sc-publish` SHA is recorded.
- Canonical sync followed by canonical `--dry-run` reports exact parity.
- The 31-path audit has a requirement and activation/proof reason for every
  path.
- Every discovered shared closure gap has an upstream issue or accepted
  resolution; there is no ATM workaround.

## Required Validation

```bash
PUBLISH_KIT_SOURCE=/Users/randlee/Documents/github/sc-publish
ATM_WORKTREE="$PWD"
bash "$PUBLISH_KIT_SOURCE/docs/publish-kit/sync-overlay.sh" --dry-run "$ATM_WORKTREE"
```

## Required Document Updates

- Record the accepted source SHA and upstream issue/PR identifiers here.
- Update the 31-path supporting audit only when the upstream owned-path list
  changes.

## Risks And Watchouts

An in-sync file diff is source parity, not a runtime proof. Do not represent
it as release readiness.
