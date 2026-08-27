# AT.1 — Canonical Consumer Install, Parity, And 1.4.3 Publish

```yaml
plan_type: sprint_plan
phase: AT
sprint: AT.1
worktree: feature/pat-s1-install-and-publish
branch: feature/pat-s1-install-and-publish
status: proposed
estimated_scope: shared-package install, parity, and authorized 1.4.3 TestPyPI/PyPI publication
```

## Goal

Install one immutable `sc-publish` package revision into an ATM worktree with
complete explicit JSON, prove that the resulting preflight and release paths
use identical shared assets and tool bootstrap, then publish the immutable
1.4.3 release to TestPyPI and (with contemporaneous authorization) PyPI.

## Deliverables

1. Record the pinned immutable `sc-publish` commit
   (`42e0fcea23f730fae0ef3d08b060cd4df6a2602e`, see phase README) and package
   digest (algorithm per the phase README Receipt Convention) in the ATM
   consumer input/evidence. Run the packaged installer and dry-run FROM the
   pinned checkout — `<sc-publish-checkout>/plugins/sc-publish/install.py` —
   never the vendored copy the installer places at the consumer repo root
   (its `PACKAGE_ROOT` resolves to the consumer repo; running it is
   destructive — upstream issue sc-publish#46). Do not copy, modify, or
   regenerate individual shared files by hand.
2. Author the complete ATM-owned `release/sc-publish-consumer-input.json` for
   all release crates, Python distributions, binaries, channels, publish
   order, and version source. Every crate participating in a release
   artifact must be declared, **including `publish = false` crates that feed
   a released artifact** (e.g. `atm-graft-python` feeding the `atm-graft`
   wheel, and symmetrically `atm-query-python`), per the existing manifest
   convention — the Ground Truth internal-crates table below is the
   reference. Rendered manifests must name every declared distribution and
   support Maturin and setuptools builds through their declared build
   system. The authoritative input schema is `load_install_values` in
   `plugins/sc-publish/install.py` at the pinned revision; a complete worked
   example exists at
   `docs/plans/phase-at/recovered/sc-publish-consumer-input.rehearsal.json`.
   Promote that recovered consumer input after reviewing its diff against the
   Publish Surface Ground Truth below and re-running the scripted
   install/dry-run/parity/test validation at the new pin. Full re-derivation
   of the document is intentionally dropped as redundant bureaucracy.
3. Use the package's single tool bootstrap in both preflight and release.
   Remove no legacy workflow until the new AT.2 deletion sprint's coverage gate; where the installer
   overwrites a same-named legacy file, the overwrite is the adoption. The
   complete expected-overwrite list at the pinned revision (per rehearsal):
   `.github/workflows/release.yml`, `.github/workflows/release-preflight.yml`,
   `.claude/agents/publisher.md`, and `release/publish-artifacts.toml`. The
   AT.1 receipt lists **every** file the installer created or overwrote (49
   files at the pin), including the installed agent files. The verified
   package digest is `75da9d18426eacb92bab3e02bb6655a35e14f69deafe1ba2d00f4e93aabc0a5b`.
4. Lock all released crates and Python deliverables to the workspace version.
   Python metadata may derive its version dynamically only from the same
   workspace version; wheel filenames remain valid PEP 440 versions and never
   use an ATM `-beta` suffix. The enforcement mechanism is the consumer
   input's `project.workspace_toml` declaration (`Cargo.toml`, i.e.
   `[workspace.package].version`): it is the single version source all
   crates and distributions must resolve to, verified by
   `.just/check_version_sync.py`.
5. Enumerate the per-channel GitHub secrets and environments the kit
   requires, from the vendored `release/publish-channel-contracts.toml` at
   the pinned revision (authoritative list; fixed cross-repo names):
   repository secrets `CARGO_REGISTRY_TOKEN`, `HOMEBREW_TAP_TOKEN`,
   `WINGET_GITHUB_TOKEN`, `SCOOP_BUCKET_TOKEN`; environment secrets
   `PYPI_API_TOKEN` (environment `pypi`) and `TEST_PYPI_API_TOKEN`
   (environment `testpypi`); environments `crates-io`, `pypi`, `testpypi`.
   Record each one's configured/missing state in the AT.1 receipt as
   fail-closed evidence, satisfying REQ-P-RELEASE-006. A missing entry is
   evidence, not a blocker (phase README Non-Blocking Outcomes table).
6. Enumerate the installed kit workflows' job/check names against the
   branch-protection required checks configured on `integrate/phase-at` and
   `develop`; update the protection settings (or record the exact transition
   plan) in the same PR so no required check names a job that no longer
   exists after the install.
7. Update the CONSUMER-OWNED lint/test expectations that assert the shape of
   installer-overwritten workflows, so the sprint branch CI passes:
   `.just/lint-config.toml` (its `[version_sync.release_wiring]`
   `required_fragments` hardcode legacy `update-homebrew:` fragments of
   `release.yml`; `.just/check_version_sync.py` cannot pass otherwise) and
   the six failing assertions in `.just/tests/test_release_preflight.py` and
   `.just/tests/test_release_homebrew_workflow.py`. These are ATM-owned
   validation data (ADR-050), NOT kit files — updating them is permitted and
   required. Retention-vs-deletion decisions about those test files still
   belong to the new AT.2 deletion sprint.

The consumer input must express Python build ownership explicitly; the shared
workflow chooses its command from this contract rather than guessing from a
package name or a null path:

```json
{
  "python_distributions": [
    {
      "name": "native-package",
      "build_system": "maturin",
      "cargo_manifest": "crates/native-package/Cargo.toml",
      "sdist": true,
      "wheels": ["ubuntu-latest", "macos-latest", "windows-latest"]
    },
    {
      "name": "pure-python-package",
      "build_system": "setuptools",
      "sdist": true,
      "wheels": ["ubuntu-latest", "macos-latest", "windows-latest"]
    }
  ]
}
```

`maturin` is valid only with a `cargo_manifest`; the shared workflow builds
the setuptools form with its declared Python build backend. A missing or
unsupported `build_system` is a manifest validation failure. `hermes-atm`
(setuptools) and the Maturin-built native bindings must both be declared and
both build through this contract.

## Publish Surface Ground Truth

Deliverable 2's authored JSON must be diffed against this enumeration,
derived from the workspace via `cargo metadata --no-deps --format-version 1`
(package names, `publish` flags, and bin targets) plus the three
`pyproject.toml` files. This table is the authoring checklist; `cargo
metadata` at sprint time is authoritative if the workspace has changed.

| Surface | Members |
| --- | --- |
| Publishable crates (crates.io), dependency order | 12 crates, one valid topological order: `atm-error` → `atm-storage` → `agent-team-mail-core` → `atm-storage-rusqlite` → `atm-http-runtime` → `atm-runtime` → `atm-template-sc-compose` → `atm-daemon-bootstrap` → `atm-daemon-client` → `agent-team-mail` → `atm-daemon` → `atm-graft`. Any order satisfying the workspace dependency graph is acceptable. |
| Internal `publish = false` crates | 9 crates, never published: `atm-architecture`, `atm-storage-sqlserver-proof`, `atm-runtime-test-support`, `atm-peer-tls-interop`, `atm-graft-python`, `atm-query-python`, `sc-lint-attributes`, `sc-lint-directives`, `sc-lint-boundary`. |
| Python distributions | `atm-graft` (maturin, `crates/atm-graft-python`, version dynamic from Cargo.toml), `atm-query` (maturin, `crates/atm-query-python`), `hermes-atm` (setuptools, `crates/hermes-atm` — a pure-Python package, not a workspace crate). |
| Released binaries | `atm` (crate `agent-team-mail`) and `atm-daemon` (crate `atm-daemon`), both confirmed cargo bin targets. `atm_post_send_hook_fixture` and `atm-daemon-benchmark` are internal bins and are not released. |

Expected channel enablement (all six):

| Channel | Expected enablement |
| --- | --- |
| crates.io | Enabled — the 12 publishable crates above, in dependency order. |
| GitHub Releases | Enabled — both binaries across the declared release targets. |
| PyPI | Enabled — all three Python distributions; TestPyPI as the rehearsal target. |
| Homebrew | Enabled — tap `randlee/homebrew-tap` formulas. |
| Winget | Enabled — identifier `randlee.agent-team-mail`. |
| Scoop | Enabled — bucket `randlee/scoop-bucket`; kit-new channel with no legacy workflow. |

## Prerequisites

- A clean ATM worktree off `develop` (baseline `d610b4c07`, post-PR #960) and
  an immutable, locally available `sc-publish` revision resolving #39–#41.
  Network unavailability may delay selecting a newer revision, but does not
  justify a local fork or hand edit.
- The caller has reviewed the complete consumer JSON before installation.

## Dependencies

- The new `AT.2` deletion sprint must follow this sprint; its coverage gates
  consume this sprint's install, parity, and publication receipts.
- No other phase dependency. This sprint is parallel-safe with product work
  because it changes only publishing assets and release metadata.

## Non-Goals

- No product, daemon, Hermes, or crate behavior change.
- No legacy-surface deletion; the new AT.2 owns that decision after the
  publication receipts exist.
- No copied shared-file modification, local publishing framework, or
  repo-specific branch in `sc-publish`.

## Paths To Delete

None. Existing release assets remain until this sprint proves the installed
shared path and publication coverage; the new AT.2 owns the explicit deletion
decision.

## Acceptance Criteria

- `install.py --dry-run` (run from the pinned checkout) reports no drift
  after installation.
- Every copied shared file byte-compares equal to its package source; the only
  consumer-specific outputs are the installer-designated manifests.
- The complete JSON declares all required channels — crates.io, GitHub
  Releases, PyPI, Homebrew, Scoop, and winget, per the Ground Truth channel
  table — and preserves the legacy package names `agent-team-mail` and
  `agent-team-mail-core`; manifest rendering fails if any required channel or
  legacy name is missing (REQ-P-RELEASE-001/002/003/005).
- The complete JSON and parsed manifests agree on publish surface, explicit
  order, all Python distributions, and each declared build system.
- No branch-protection required check on `integrate/phase-at` or `develop`
  references a job name that no longer exists after the install
  (Deliverable 6).
- A fixture containing both the `maturin` and `setuptools` forms proves the
  shared workflow selects the declared backend and rejects an absent or
  unsupported build system before any upload.
- Preflight and release resolve the same package revision, manifest digest,
  and tool bootstrap; tests fail if either path selects a different source.
- Version-sync validation proves all released crates and Python distributions
  resolve to one workspace version.

## Required Validation

All kit commands run from the pinned `sc-publish` checkout, never the
vendored consumer-root `install.py` (sc-publish#46; see Deliverable 1). Each
command below must exit 0 on a clean install.

```bash
<venv>/bin/python <sc-publish-checkout>/plugins/sc-publish/install.py --dry-run --input release/sc-publish-consumer-input.json <worktree>
# Package-byte parity: byte-compare every copied kit file against its package
# source (the rehearsal's sweep: same file set and exclusion rules as
# install.py; expected result "byte-identical: 49, mismatched/missing: 0").
# A plain `diff -ru` is acceptable only if it excludes ALL consumer-owned
# files under release/ (publish-artifacts.toml, publish-channel-contracts.toml,
# sc-publish-consumer-input.json, and any other consumer-owned release/ file)
# so that it exits 0 on a clean install.
python3 .github/scripts/release_artifacts.py validate-manifest --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml
python3 .just/check_version_sync.py
```

Run the package's own installer/unit-test suite (the mixed maturin/setuptools
fixture and the unsupported-build-system rejection tests live in that suite)
with the exact command proven by the rehearsal, from the `sc-publish`
checkout at the pinned revision — not from the consumer:

```bash
<venv>/bin/python -m pytest <sc-publish-checkout>/plugins/sc-publish/.github/scripts/tests/ -q
```

Its pass at the pin is recorded in the AT.1 receipt. ATM does not duplicate
that shared test suite.

## Required Document Updates

- Record the immutable `sc-publish` revision, package digest, complete
  consumer-input digest, and generated-manifest digests in the AT.1 receipt
  at `docs/plans/phase-at/receipts/AT.1-receipt.md` (shape per the phase
  README's Receipt Convention), and update the phase README pin.
- Record any shared-package defect as an upstream `sc-publish` issue/PR with
  the failing fixture and command; do not patch its copied ATM files
  (ADR-050).

## Rollback

If the installed kit path cannot pass Required Validation and the defect is
upstream (in the shared package, not the consumer input):

1. File the upstream `sc-publish` issue with the failing fixture and command.
2. Then either wait for the upstream fix and re-pin (restarting AT.1's
   proof), or `git revert` the install commits on the sprint branch —
   restoring the legacy workflows byte-for-byte. Never hand-edit a copied
   shared file to limp past the failure.
3. Do not merge the sprint PR in this state; escalate to team-lead.
4. If the sprint is abandoned, the sprint branch is retired through the
   standard worktree-abort flow (`sc-worktree-abort`). `develop` is never
   touched.

## Risks And Watchouts

- A package or installer defect is reported and fixed in `sc-publish`; AT.1
  must then repeat the install and proof against the new immutable revision.
- Upstream moves fast; adopting a newer revision restarts AT.1's proof and is
  never a reason to hand-merge changes.
- A credential result is not a package-install failure and is not grounds for
  inventing a local credential workaround.
- Classify every non-passing result against the phase README's Non-Blocking
  Outcomes table before reacting; only its GENUINE STOP row halts work.
## Branch And Review Mechanics For Publication

Workflow dispatches run against the immutable `main` tag and its release
assets and change no source. All sprint artifacts — receipts and doc-status
updates — land on `feature/pat-s1-install-and-publish` and merge through a normal
PR under the quality-mgr gate (phase README Branch Strategy). The sprint
never commits to `main`.

**Dispatch mechanics.** `gh workflow run` executes the workflow YAML from the
ref given to `--ref`, not from the release tag. This sprint therefore dispatches
with `--ref integrate/phase-at` — the branch where the installed kit workflows
live after this sprint's PR merges — while the immutable release
tag is passed as a workflow input. The kit's `pypi-publish.yml`
`workflow_dispatch` inputs at the pinned revision are `tag` (the published
GitHub Release tag) and `target` (choice: `testpypi` | `production`); the
workflow checks out `inputs.tag` internally and downloads the release's
already-attached assets. The concrete invocations:

```bash
gh workflow run pypi-publish.yml --ref integrate/phase-at -f tag=v1.4.3 -f target=testpypi
# after contemporaneous production authorization (see Prerequisites):
gh workflow run pypi-publish.yml --ref integrate/phase-at -f tag=v1.4.3 -f target=production
```

No local checkout of `main` is required at any point: the sprint performs
dispatch plus public-index verification only.

## Publication Deliverables

1. Resolve the immutable `main` commit, version, signed/tagged release assets,
   the pinned package revision, consumer-input digest, manifest digest, and build
   receipt before any upload. Reject an asset/version/digest mismatch.
2. Before the first TestPyPI/PyPI dispatch, verify the `hermes-atm` and
   `atm-graft` PyPI descriptions/READMEs and the GitHub release notes carry
   the first-public-release framing decided in
   [ADR-049](../../adr/ADR-049-hermes-atm-first-public-pypi-release-versioning.md);
   record the check in the receipt.
3. Read the new pin's release-candidate provenance gate (`sc-publish` PR #48,
   `release-candidate.yml`) and record its impact on the `pypi-publish.yml`
   dispatch preconditions in the AT.1 receipt before the first TestPyPI
   dispatch. The gate runs before release creation; `pypi-publish.yml` accepts
   only `tag` and `target`, then requires a published non-draft release with
   attached assets. The immutable retry therefore uses the post-release path
   without rebuilding or re-running provenance. AT-REPIN-VERIFY-R1 is complete:
   package digest `75da9d18426eacb92bab3e02bb6655a35e14f69deafe1ba2d00f4e93aabc0a5b`,
   49 files, 49/49 parity, dry-run clean, kit tests 81 passed/8 skipped, and
   manifest validation passed (see recovered verification receipt).
4. With the release authorizer's TestPyPI authorization on record (it may be
   granted in advance in the sprint dispatch message; quote it verbatim in
   the receipt), execute the canonical release workflow against that
   immutable source for TestPyPI. It must build/select all manifest-declared
   wheels and sdists, validate them, and perform a real retry-safe upload
   with `--skip-existing` behavior.
5. Verify TestPyPI's public package/version/file set and install each package
   from the public index on Python 3.11, 3.12, 3.13, and 3.14.
6. After contemporaneous production authorization from the release authorizer
   (quoted verbatim in the receipt; advance or standing authorization is not
   valid for production), repeat the same immutable asset and validation path
   for PyPI. Store public URLs, hashes, workflow run IDs, interpreter
   results, and the package/tool/manifest identity tuple.

## Publication Prerequisites

- The install/parity portion of this sprint's acceptance criteria passes for
  the exact package revision and input, and this sprint's PR is merged to
  `integrate/phase-at` (the dispatch `--ref`
  target).
- `main` contains the intended immutable 1.4.3 source and release assets.
- The release authorizer explicitly authorizes the TestPyPI and PyPI
  dispatches. The release authorizer is defined in the phase README: only
  the human repository owner (Rand), acting outside any agent role — never
  an agent, coordinator, or teammate. Each authorization is quoted verbatim
  in the receipt. TestPyPI authorization may be granted in advance in the
  sprint dispatch message; production authorization must be contemporaneous
  with the production dispatch.

## Publication Dependencies

- Channel actions are ordered: TestPyPI public verification must pass before
  any PyPI dispatch.
- The new `AT.2` deletion sprint must follow this sprint; this sprint's
  receipts are its coverage evidence. The TestPyPI receipt is its minimum
  gate; the Hermes legacy-workflow rows additionally require this sprint's
  production PyPI receipt.

## TestPyPI Checkpoint

TestPyPI success plus its receipt section is an independently mergeable,
QA-checkable checkpoint. If production authorization is delayed, the sprint
PR may merge with the TestPyPI receipt and an explicit
`production: pending-authorization` marker; the PyPI dispatch then lands as
a follow-up commit/PR on the same receipt file
(`docs/plans/phase-at/receipts/AT.1-receipt.md`) without reopening this sprint.

## Publication Non-Goals

- No product-source changes (sprint artifacts on the feature branch are
  receipts and doc-status updates only), no version bump, crate publication,
  GitHub Release rebuild, Homebrew, Scoop, or Winget publication, and no
  Hermes gateway/product onboarding flow. The `hermes-atm` public-index
  install/import check (pip install from TestPyPI/PyPI and module import on
  each supported interpreter) IS required — it is part of this sprint's
  verification, not Hermes onboarding.
- No token rotation, token-value logging, credential workaround, or claim that
  a GitHub-environment credential exists without workflow evidence.

## Publication Paths To Delete

None. This is publication evidence only; it deletes or replaces no workflow,
asset, tag, or release.

## Publication Acceptance Criteria

- TestPyPI and PyPI contain every Python distribution declared in the same
  immutable manifest, with exactly version 1.4.3 and matching hashes.
- Each supported Python version installs from the public index and imports the
  declared module without using local wheels, paths, or a source checkout.
- The workflow records a structured, channel-specific failure if a GitHub
  environment credential is unavailable. A channel-scoped
  credential-unavailable (fail-closed) result is valid evidence the workflow
  fails closed, and per the phase README Non-Blocking Outcomes table it is
  never an emergency — but it does NOT constitute publication: it does not
  close this sprint's publication acceptance criterion and does not satisfy
  the new AT.2 coverage-evidence prerequisite.
- The PyPI descriptions and release notes carry ADR-049's
  first-public-release framing, verified before the first dispatch
  (Deliverable 2).
- Receipts prove the same source commit, package revision, consumer-input
  digest, manifest digest, tool bootstrap, asset hashes, and validation
  results for TestPyPI and PyPI.

## Publication Required Validation

```bash
python3 .github/scripts/release_artifacts.py validate-manifest --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml
python3 .github/scripts/release_artifacts.py verify-python-release-assets --manifest release/publish-artifacts.toml --asset-dir <immutable-release-assets>
# Dispatches (see Branch And Review Mechanics: workflow YAML executes from
# --ref integrate/phase-at; the release tag is a workflow input; no local
# checkout of main):
gh workflow run pypi-publish.yml --ref integrate/phase-at -f tag=v1.4.3 -f target=testpypi
gh workflow run pypi-publish.yml --ref integrate/phase-at -f tag=v1.4.3 -f target=production
python3 -m pip install --index-url https://test.pypi.org/simple --only-binary=:all: <package>==1.4.3
python3 -m pip install --index-url https://pypi.org/simple --only-binary=:all: <package>==1.4.3
```

Run the public-index install commands in fresh Python 3.11–3.14 environments
for every manifest-declared Python package.

## Publication Required Document Updates

Append the sprint receipt at `docs/plans/phase-at/receipts/AT.1-receipt.md`
(shape per the phase README's Receipt Convention) containing the
TestPyPI/PyPI URLs, workflow run IDs, source and asset identities,
index-install matrix, and any channel-scoped fail-closed credential result.

## Publication Risks And Watchouts

- Public indexes are immutable: an existing different file/version is a hard
  stop; do not overwrite or rebuild under the same version.
- A retry only reuses the same verified immutable assets. Any changed asset or
  manifest starts a new release decision.
- Classify every non-passing result against the phase README's Non-Blocking
  Outcomes table before reacting; only its GENUINE STOP row halts work.
