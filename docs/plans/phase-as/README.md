# Phase AS — Shared Publish-Kit Migration

```yaml
plan_type: phase_index
phase: AS
status: proposed
branch: plan/sc-compose-publish-kit-migration
worktree: sc-compose-publish-kit-migration
```

## Goal

Adopt the `sc-publish` publish kit as an exact, upstream-owned overlay; prove
that its manifest-driven preflight and release path can replace the legacy ATM
release flow without reintroducing toolchain or validation drift.

## Governing boundaries

- `sc-publish` is the sole source of shared workflows, actions, scripts,
  agent prompts, and tests. ATM installs them only through the canonical
  package installer, byte-for-byte.
- ATM owns only repository data in its release manifest and explicitly
  namespaced, non-shared validation data.
- A shared-file defect is an upstream `sc-publish` issue/PR, never an ATM
  overlay patch.
- Preflight and publish consume one resolved manifest, toolchain, and
  validation receipt. `source_commit`, `manifest_sha256`, `toolchain_sha256`,
  and `validation_sha256` must agree; a digest mismatch fails closed.
- Production publishing requires explicit authorization; planning or
  preflight does not grant it.

## Phase delivery flow

This planning branch can update documentation only. It does not authorize
integration, a release, or a production channel. Once AS implementation is
authorized, work follows the established branch policy: implementation sprints
merge through `integrate/phase-AS` into `develop`; release execution then uses
an explicitly user-authorized release PR from `develop` to `main`. AS.4 and
AS.6 are release operations on immutable `main`, not an exception to that
policy.

## Current shared baseline

`sc-publish` `develop` at `d9b4588e73845cf47fe04d42c2a02b8b30136fc1` is the
promotion baseline. It relocates the shared surface into the package and
replaces the retired overlay script with `plugins/sc-publish/install.py`.

The baseline is not a claim that AS.1 is closed. Its remaining installer and
evidence refinements are listed in AS.1 as upstream-owned deliverables; ATM
must not recreate the retired script or patch the package locally.

## Sprint sequence

| Sprint | Purpose | Dependency |
| --- | --- | --- |
| [AS.1](AS.1-overlay-contract.md) | Freeze and justify the exact upstream overlay. | Start point |
| [AS.2](AS.2-consumer-manifest-contract.md) | Justify ATM consumer data and raise upstream gaps. | must_follow AS.1 |
| [AS.3](AS.3-worktree-preflight-proof.md) | Prove canonical preflight from the exact sync worktree. | must_follow AS.2 |
| [AS.4](AS.4-authorized-pypi-1.4.3.md) | Publish the already-built 1.4.3 PyPI artifacts from immutable `main`. | must_follow AS.3 |
| [AS.5](AS.5-main-migration-merge.md) | Promote the verified migration from `develop` to `main`. | must_follow AS.3 and AS.4 |
| [AS.6](AS.6-full-1.4.4-release.md) | Execute and verify the first full canonical release. | must_follow AS.5 |

The former [migration design](../publish-kit-migration/README.md) is retained
as supporting evidence only. These Phase AS sprint documents are authoritative.
