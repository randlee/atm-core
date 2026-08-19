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

- **Amended 2026-08-18 (Rand, relayed via quality-mgr):** the boundaries below
  as originally written pushed atm-core-specific publish correctness into
  `sc-publish`'s generic schema, turning every atm-core release nuance
  (dynamic Maturin versioning, non-crates.io-published bridge crates,
  publish-order, fail-closed receipts) into an upstream schema-growth request
  and a blocking wait. That is reversed as of AS.4 onward:
  - Pre-publish validation — version consistency, manifest correctness
    (including `publish_order` and non-published bridge crates), and
    release-receipt correctness — is atm-core DATA and atm-core's own concern.
    It is implemented and run directly in atm-core, by atm-core-owned
    scripts/workflows, and is never gated on a `sc-publish` schema change
    landing upstream first.
  - `sc-publish`'s installer/generator may still render a thin starter/scaffold
    manifest shape, but it does not validate or gate atm-core-specific
    correctness — that overreach (e.g. rejecting `publish_order = 0` for
    non-published bridge crates) is what made AS.2/AS.3 block on upstream in
    the first place.
  - `sc-publish` remains the right place only for mechanics that are actually
    generic across its 30+ consumers (e.g. how to talk to a registry given
    credentials, credential-rehearsal patterns). "Is atm-core's release
    correct" is never `sc-publish`'s business.
  - A finding that atm-core's own validation/workflow logic is wrong is an
    atm-core fix, fixed directly here, same-day — not an upstream ticket that
    blocks the sprint.
- `sc-publish` is the source of genuinely shared, generic release mechanics
  installed through the canonical package installer, byte-for-byte — scoped
  per the amendment above, not as the sole source of atm-core's publish
  correctness.
- Preflight and publish still consume one resolved manifest and toolchain, but
  the validation receipt proving atm-core's release is correct is produced by
  atm-core's own validation, not contingent on an external `sc-publish`
  receipt-mechanism PR merging first.
- Production publishing requires explicit authorization; planning or
  preflight does not grant it.
- A blocking finding on one work item halts only that item; independent work
  continues in parallel rather than idling the whole sprint.

## Phase delivery flow

This planning branch can update documentation only. It does not authorize
integration, a release, or a production channel. Once AS implementation is
authorized, work follows the established branch policy: implementation sprints
merge through `integrate/phase-AS` into `develop`; release execution then uses
an explicitly user-authorized release PR from `develop` to `main`. AS.4 and
AS.6 are release operations on immutable `main`, not an exception to that
policy.

## Current shared baseline

`sc-publish` `develop` at `240fd52` is the promotion baseline. It provides the
explicit-input, pinned-bootstrap installer and replaces the retired overlay
script with `plugins/sc-publish/install.py`.

The baseline is not a claim that AS.1 is closed. Its remaining installer and
evidence refinements are listed in AS.1 as upstream-owned deliverables; ATM
must not recreate the retired script or patch the package locally.

## Sprint sequence

| Sprint | Purpose | Dependency |
| --- | --- | --- |
| [AS.1](AS.1-overlay-contract.md) | Freeze and justify the exact upstream overlay. | in_progress |
| [AS.2](AS.2-consumer-manifest-contract.md) | Justify ATM consumer data and raise upstream gaps. | must_follow AS.1 |
| [AS.3](AS.3-worktree-preflight-proof.md) | Prove canonical preflight from the exact sync worktree. | must_follow AS.2 |
| [AS.4](AS.4-authorized-pypi-1.4.3.md) | Cut and publish a fresh `1.4.4` PyPI-only release (ADR-049 disclosure + current manifest baked in; the already-built `1.4.3` artifacts predate both and cannot be republished as-is). | must_follow AS.3 |
| [AS.5](AS.5-main-migration-merge.md) | Promote the verified migration from `develop` to `main`. | must_follow AS.3 and AS.4 |
| [AS.6](AS.6-full-1.4.4-release.md) | Execute and verify the first full canonical release. | must_follow AS.5 |

The former [migration design](../publish-kit-migration/README.md) is retained
as supporting evidence only. These Phase AS sprint documents are authoritative.
