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
  agent prompts, and tests. ATM copies them only through its canonical sync
  script, byte-for-byte.
- ATM owns only repository data in its release manifest and explicitly
  namespaced, non-shared validation data.
- A shared-file defect is an upstream `sc-publish` issue/PR, never an ATM
  overlay patch.
- Preflight and publish consume one resolved manifest, toolchain, and
  validation receipt. A digest mismatch fails closed.
- Production publishing requires explicit authorization; planning or
  preflight does not grant it.

## Sprint sequence

| Sprint | Purpose | Dependency |
| --- | --- | --- |
| [AS.1](AS.1-overlay-contract.md) | Freeze and justify the exact upstream overlay. | Start point |
| [AS.2](AS.2-consumer-manifest-contract.md) | Justify ATM consumer data and raise upstream gaps. | must_follow AS.1 |
| [AS.3](AS.3-worktree-preflight-proof.md) | Prove canonical preflight from the exact sync worktree. | must_follow AS.2 |
| [AS.4](AS.4-authorized-pypi-1.4.3.md) | Publish the already-built 1.4.3 PyPI artifacts from immutable `main`. | must_follow AS.3 |
| [AS.5](AS.5-main-migration-merge.md) | Merge the verified migration to `main`. | must_follow AS.3 and AS.4 |
| [AS.6](AS.6-full-1.4.4-release.md) | Execute and verify the first full canonical release. | must_follow AS.5 |

The former [migration design](../publish-kit-migration/README.md) is retained
as supporting evidence only. These Phase AS sprint documents are authoritative.
