# AS.2 — Justify ATM consumer-manifest activation

```yaml
plan_type: sprint_plan
phase: AS
sprint: AS.2
worktree: sc-compose-publish-kit-migration
branch: plan/sc-compose-publish-kit-migration
status: proposed
estimated_scope: manifest data and upstream-contract review
```

## Goal

Make ATM a correct data-only consumer of the canonical publish kit without
embedding workflow, credential, toolchain, or repository-specific logic in
the shared surface.

## Scope Summary

For every ATM artifact or channel declaration, record the canonical consumer,
activation state, and required proof. Raise gaps upstream instead of creating
an ATM adapter, wrapper, or workflow fork.

## Governing Requirements

- Channels, artifacts, binaries, destinations, and validations are manifest
  data.
- Credential names, environments, endpoints, and liveness rules are in the
  shared channel contract.
- The one shared bootstrap resolves all tools identically for preflight and
  publish.
- Maturin Python distributions derive their release version from their
  corresponding Cargo package through PEP 621 `dynamic = ["version"]`.
  `hermes-atm` is a pure setuptools package with no Cargo package, so its
  literal `[project].version` must equal the workspace numeric release base
  and be checked by the shared version-lock validator; a dummy Cargo package
  is prohibited.
- One release-version contract governs every crate, wheel, binary, generated
  manifest, archive, and channel declaration. Stable releases use the exact
  same `X.Y.Z` everywhere. For a prerelease Cargo version such as
  `X.Y.Z-beta-AS`, Python wheels use the explicitly derived base `X.Y.Z`
  because they cannot carry that `-beta…` suffix; this is the sole permitted
  version projection.

## Governing ADRs

- [ADR-049](../../adr/ADR-049-hermes-atm-first-public-pypi-release-versioning.md)
  and [ADR-050](../../adr/ADR-050-shared-publish-kit-ownership.md).

## Governing Boundaries

- ATM may change its artifact manifest after the required shared schema exists.
- ATM must not alter a copied helper, workflow, action, agent, or test.

## Prerequisites

- AS.1 accepted source/parity record and upstream-gap disposition.

## Hard Dependencies

- `AS.1`: `must_follow`; merge AS.1 planning updates before each AS.2 round.
- `AS.3`: `must_follow`; real preflight requires the accepted manifest contract.

## Non-Goals

- Local parser accommodations for valid dynamic PEP 621 metadata.
- Local `extra_validations` runner or legacy-helper wrapper.
- Enabling a destination merely because its generic workflow exists.

## Sub-Tasks

1. Map every ATM artifact and enabled channel to one canonical manifest field,
   canonical consumer, and proof command/receipt.
2. Keep undeclared Homebrew, Scoop, winget, and other channels dormant with a
   recorded reason; do not activate from workflow presence alone.
3. Confirm PF-1 uses the existing non-disclosing GitHub
   variable/credential-verification workflow.
4. Confirm PF-2 consumes the existing crates.io fenced JSON receipt for every
   manifest crate: versions, last-publish timestamp, classification, and
   planned action.
5. Confirm PF-3 handles partial state: publish missing target artifacts, skip
   already-at-target artifacts, block true version conflicts. A version bump
   remains an explicit release decision.
6. Supply the shared installer one complete consumer JSON document containing
   explicit artifacts, crate publish order, binaries, wheels, and per-channel
   enablement. Reject any install request that omits that document; source
   discovery is advisory `--example-json` output only.
7. Submit any missing generic schema/support to `sc-publish`; wait for its
   accepted implementation and exact sync.
8. Preserve `atm-graft-python`'s Maturin/Cargo-derived PEP 621 metadata and
   convert `atm-query-python` to the same supported dynamic path. Extend
   `.just/check_version_sync.py` to recognize that declared Cargo source. Keep
   the pure setuptools `hermes-atm` literal version locked to the workspace
   numeric release base; do not invent a Cargo package solely to manufacture
   dynamic metadata.
9. Extend the shared release-version receipt and ATM version-lock test to
   enumerate every deliverable. It must reject any mismatch except the
   declared prerelease-to-wheel base-version projection above.

## Split Recommendation

Keep AS.2 data-contract work separate from real dispatch. AS.3 owns execution
evidence and must not silently add schema behavior.

## Acceptance Criteria

- Every enabled ATM artifact/channel maps to a canonical consumer and proof.
- Every dormant channel has an explicit manifest-state reason.
- Installation cannot infer or enable an artifact/channel not present in the
  consumer’s supplied manifest input.
- Installing from the ATM JSON produces every package-owned shared file and
  both rendered release manifests; byte/source parity and semantic manifest
  equality are checked before the clean dry-run proof.
- PF-1, PF-2, and PF-3 use existing canonical solutions as defined above.
- Each released Python distribution is locked to the workspace release base:
  both Maturin packages use their supported Cargo-derived path, while the pure
  setuptools `hermes-atm` literal is checked by the same version-lock test.
- The release receipt enumerates every crate, wheel, binary, archive, and
  manifest version. Stable releases are exactly equal; prerelease wheels may
  differ only by the declared removal of the `-beta…` suffix.
- No ATM-only shared-code adaptation exists.
- Every unmet generic capability has an upstream tracking item, not a local
  workaround.

## Required Validation

```bash
python3 scripts/release_artifacts.py validate-manifest \
  --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml
python3 scripts/release_artifacts.py preflight-secret-plan \
  --manifest release/publish-artifacts.toml
python3 .just/check_version_sync.py
```

## Required Document Updates

- Update the ATM artifact/channel manifest comments and this sprint’s mapping.
- Link upstream issues/PRs for generic contract gaps.

## Risks And Watchouts

Do not apply a one-shape PEP 621 policy to a package that has no Cargo
metadata. The invariant is one locked release version, not a dummy package or
an unsupported dynamic-version mechanism.
