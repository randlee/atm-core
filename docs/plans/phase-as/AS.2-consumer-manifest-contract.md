# AS.2 — Justify ATM consumer-manifest activation

```yaml
plan_type: sprint_plan
phase: AS
sprint: AS.2
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pas-s2-consumer-manifest-contract
branch: feature/pas-s2-consumer-manifest-contract
status: in_progress
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
2. Keep inactive Homebrew, Scoop, winget, and other channels dormant with a
   recorded authorization reason; do not activate declared channel data from
   workflow presence alone.
3. Confirm PF-1 uses the existing non-disclosing GitHub
   variable/credential-verification workflow.
4. Confirm PF-2 consumes the existing crates.io fenced JSON receipt for every
   manifest crate: versions, last-publish timestamp, classification, and
   planned action.
5. Confirm PF-3 handles partial state: publish missing target artifacts, skip
   already-at-target artifacts, block true version conflicts. A version bump
   remains an explicit release decision.
6. Supply the shared installer one complete consumer JSON document containing
   explicit artifacts, crate dependency/publish order, binaries, wheels, and per-channel
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
  equality are checked before the clean dry-run proof. The source-layout and
  complete-schema item formerly tracked by
  [`sc-publish` #17](https://github.com/randlee/sc-publish/issues/17) closed
  through PR #24 at `68c06f97`; AS.3 owns the resulting unchanged-sync
  execution proof. No ATM-side translation or workaround is permitted.
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
python3 .github/scripts/release_artifacts.py validate-manifest \
  --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml
python3 .github/scripts/release_artifacts.py preflight-secret-plan \
  --manifest release/publish-artifacts.toml
python3 .just/check_version_sync.py
```

## Required Document Updates

- Update the ATM artifact/channel manifest comments and this sprint’s mapping.
- Link upstream issues/PRs for generic contract gaps.

## ATM Consumer Mapping And Activation Record

This is ATM-owned data. It does not change any copied publish-kit asset.
The canonical installer must render this declared contract unchanged once its
complete schema is available.

| Deliverable | Canonical manifest field | Canonical consumer / proof |
| --- | --- | --- |
| `atm-error` | `[[crates]]` (`publish_order = 1`) | crates.io root release; `validate-publish-order` and PF-2 receipt |
| `atm-storage` | `[[crates]]` (`publish_order = 2`) | crates.io root release; `validate-publish-order` and PF-2 receipt |
| `agent-team-mail-core` | `[[crates]]` (`publish_order = 3`) | crates.io root release; `validate-publish-order` and PF-2 receipt |
| `atm-storage-rusqlite` | `[[crates]]` (`publish_order = 4`) | crates.io root release; `validate-publish-order` and PF-2 receipt |
| `atm-http-runtime` | `[[crates]]` (`publish_order = 5`) | crates.io root release; `validate-publish-order` and PF-2 receipt |
| `atm-daemon-client` | `[[crates]]` (`publish_order = 6`) | crates.io root release; `validate-publish-order` and PF-2 receipt |
| `atm-runtime` | `[[crates]]` (`publish_order = 7`) | crates.io root release; `validate-publish-order` and PF-2 receipt |
| `atm-template-sc-compose` | `[[crates]]` (`publish_order = 8`) | crates.io root release; `validate-publish-order` and PF-2 receipt |
| `atm-daemon-bootstrap` | `[[crates]]` (`publish_order = 9`) | crates.io root release; `validate-publish-order` and PF-2 receipt |
| `atm-daemon` | `[[crates]]` (`publish_order = 10`) | crates.io root release; `validate-publish-order` and PF-2 receipt |
| `atm-graft` | `[[crates]]` (`publish_order = 11`) | crates.io root release; PF-2 receipt |
| `agent-team-mail` | `[[crates]]` (`publish_order = 12`) and `[[release_binaries]]` | crates.io root release and release archive; PF-2 receipt and artifact verification |
| `atm-graft-python` / `atm-graft` | `[[python_packages]]` plus `[[python_distributions]]` | PyPI wheel/sdist matrix; Cargo-derived PEP 621 version check |
| `atm-query-python` / `atm-query` | `[[python_packages]]` plus `[[python_distributions]]` | PyPI wheel/sdist matrix; Cargo-derived PEP 621 version check |
| `hermes-atm` | `[[python_packages]]` | PyPI wheel/sdist matrix; literal PEP 621 version-lock check |
| `atm`, `atm-daemon` | `[[release_binaries]]`, `[[release_targets]]`, and archive fields | GitHub Release archive build and verification receipt |
| generated artifact/channel manifests | canonical template outputs | installer semantic-equality proof plus a clean `--dry-run` |

### Channel State

| Channel | State | Reason / activation proof |
| --- | --- | --- |
| crates.io | dormant until AS.6 authorization | The twelve declared crates need PF-2/PF-3 classification and an authorized full release; workflow presence is not authorization. |
| GitHub Release | dormant until AS.6 authorization | The binary archives are an AS.6 deliverable and require the matched AS.3 receipt. |
| PyPI | dormant except for AS.4’s separately authorized immutable `1.4.3` operation | AS.4 authorizes only the declared Python artifacts after matched AS.3 evidence; AS.6 controls the next full release. |
| Homebrew | dormant | The consumer manifest declares the tap, formulas, assets, and workflow; dispatch remains gated until an authorized release receipt permits this channel. |
| Scoop | dormant | The consumer manifest declares the bucket, manifest template, binary, and workflow; dispatch remains gated until an authorized release receipt permits this channel. |
| winget | dormant | Existing package metadata is version-locked, but no channel authorization is declared. |

### PF And Shared-Capability Record

| Item | Canonical solution | AS.2 disposition |
| --- | --- | --- |
| PF-1 credential/configuration preflight | `release-preflight.yml` invokes the shared non-disclosing `preflight-secret-plan`; channel contracts carry names and liveness kinds, never values. | Use unchanged. |
| PF-2 registry state | `public-registry-check-plan` provides the fenced crates.io JSON inquiry plan for every declared crate. | Use unchanged; retain each artifact’s version, last-publish timestamp, classification, and planned action in the receipt. |
| PF-3 partial state | Existing registry classification distinguishes missing, already-at-target, and true-conflict artifacts. | Publish missing targets, skip already-at-target targets, and block conflicts; a version bump is an explicit release decision. |
| Full installer schema and render/validate parity | [`sc-publish` #17](https://github.com/randlee/sc-publish/issues/17), closed by PR #24 `68c06f97` | Accepted upstream; AS.3 must prove the unchanged installer renders `[project]`, channel configuration, and Python package/distribution fields before release dispatch. |
| Generic Maturin/Python version-model contract | [`sc-publish` #13](https://github.com/randlee/sc-publish/issues/13) | The broader contract is Maturin `dynamic = ["version"]` from an explicit Cargo source, alongside literal-version pure setuptools packages. ATM declares both shapes; the two concrete validator paths below remain separately tracked. |
| `validate-manifest` dynamic-version resolution | [`sc-publish` #20](https://github.com/randlee/sc-publish/issues/20) | Blocked upstream until the canonical `[[python_packages]]` validation resolves a Maturin dynamic version from its declared Cargo source. This is distinct from #13's broader contract and from #21's Cargo lockstep path. |
| `verify-version-lockstep` literal bridge-version allowance | [`sc-publish` #21](https://github.com/randlee/sc-publish/issues/21) | Blocked upstream until canonical lockstep accepts an intentionally literal Maturin bridge version when it equals the documented workspace release base. This is distinct from #20's PEP 621 validation path. |
| Fail-closed version receipt | [`sc-publish` #11](https://github.com/randlee/sc-publish/issues/11) | Blocked upstream: AS.3 cannot dispatch until the shared receipt enumerates and binds all deliverables with source, manifest, toolchain, and validation digests. |
| Legacy local release validation split path | AS.5 retirement target | `just validate` invokes `scripts/validate_release.py`, which calls the narrower legacy `scripts/release_artifacts.py`; it can pass while CI's canonical `.github/scripts/release_artifacts.py` fails. Do not treat local `just validate` success as canonical release validation evidence; AS.5 owns retirement, not an ATM-side adapter. |

## Risks And Watchouts

Do not apply a one-shape PEP 621 policy to a package that has no Cargo
metadata. The invariant is one locked release version, not a dummy package or
an unsupported dynamic-version mechanism.

## Current Installer Execution Record

[`evidence/AS.2-consumer-input.json`](evidence/AS.2-consumer-input.json) is
the explicit, caller-owned ATM input contract. It declares all twelve
publishable crates, three Python packages, both release binaries, and every
channel's disabled state. It is intentionally not yet the terminal AS.2
consumer document: the shared template currently renders wheel entries as
`[[artifacts.wheels]]`, while ATM's canonical release validator consumes
`[[python_packages]]` and `[[python_distributions]]`. The current canonical
installer also rejects ATM's explicit non-published bridge crates because their
`publish_order = 0` values are not yet an accepted generic input shape. Both
the shape and rendered-manifest parity were closed by
[`sc-publish` #17](https://github.com/randlee/sc-publish/issues/17) through
PR #24 at `68c06f97`. AS.3 owns unchanged-sync execution proof; no ATM-side
translation or workaround is permitted.

`python3 .just/tests/test_as2_consumer_contract.py` prevents ATM-owned input
drift: every declared crate must still resolve to its stated Cargo package,
publish order remains contiguous, and all Python, binary, and channel entries
remain explicit. It intentionally does not duplicate shared rendering or
validation logic.
