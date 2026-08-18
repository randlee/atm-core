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
- The canonical `plugins/sc-publish/install.py` package installer is the only
  copier and parity checker. The prior standalone overlay script is retired.
- No copied path may be locally edited.

## Governing ADRs

- No new ADR. This sprint preserves the existing repository ownership
  boundary rather than changing an architecture decision.

## Governing Boundaries

- Shared paths: upstream-owned and synchronized unchanged.
- ATM paths: manifest data and namespaced extension data only.

## Prerequisites

- `sc-publish` promotion baseline
  `d9b4588e73845cf47fe04d42c2a02b8b30136fc1` is reachable.
- The canonical package installer is available from that source.

## Hard Dependencies

- `AS.2`: `must_follow`; it may only justify consumer data after AS.1 records
  the shared ownership contract.

## Non-Goals

- Editing shared overlay files in ATM.
- Activating publication channels.
- Repairing upstream shared behavior locally.

## Sub-Tasks

1. Record the accepted source SHA and execute the canonical package installer
   twice: once to synchronize and once in `--dry-run` mode to prove no
   remaining byte difference.
2. Retain the 31-path audit in the supporting migration design as the
   complete decision record. The overlay is atomic: no individual path is
   selected, omitted, or patched locally.
3. File upstream requirements for every shared closure gap discovered by the
   audit: relative-dependency closure, installed-archive-member validation,
   resolved release-plan evidence, `extra_validations`, Cargo-derived dynamic
   PEP 621 versions, and an explicit installer-input contract.
4. Require an upstream closure test that fails whenever a copied shared file
   references an unvendored relative dependency not declared as a generic
   consumer interface.
5. Require the shared installer to consume complete explicit artifact/channel
   input. Source discovery may produce an example only; it must never infer a
   production publish surface, publish order, or enabled destination.
6. Require the shared bootstrap to provision the exact `sc-compose` CLI used
   by install/render and run the semantic installer integration test in CI
   through that bootstrap.
7. Require the installer’s sole consumer input to be complete declared data:
   source/version policy, artifacts, explicit crate order, wheels, binaries,
   channels, and channel settings. It must reject missing production input;
   discovery is allowed only for an advisory example command.

## Split Recommendation

Keep AS.1 isolated: it resolves ownership and evidence contracts only. Any
consumer manifest work belongs to AS.2.

## Acceptance Criteria

- The `d9b4588` baseline and the accepted promotion SHA are recorded.
- Canonical installation followed by canonical `--dry-run` reports exact
  parity from one complete declared consumer input.
- The 31-path audit has a requirement and activation/proof reason for every
  path.
- Every discovered shared closure gap has an upstream issue or accepted
  resolution; there is no ATM workaround.
- The upstream installer has no enabled-channel defaults and no production
  source-discovery fallback.
- The upstream semantic installation test passes in CI using the same pinned
  toolchain that consumers receive.

## Required Validation

```bash
PUBLISH_KIT_SOURCE=/Users/randlee/Documents/github/sc-publish
ATM_WORKTREE="$PWD"
git -C "$PUBLISH_KIT_SOURCE" rev-parse d9b4588
python3 "$PUBLISH_KIT_SOURCE/plugins/sc-publish/install.py" --help
```

## Required Document Updates

- Record the accepted source SHA and upstream issue/PR identifiers here.
- Update the 31-path supporting audit only when the upstream owned-path list
  changes.

## Risks And Watchouts

The baseline installer still needs the explicit-input and shared-toolchain
refinements listed above. Do not represent its current unit-test pass or a
future in-sync file diff as release readiness.
