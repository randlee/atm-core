# AT.1 Receipt — Canonical Consumer Install, Parity, And Publication Readiness

```yaml
receipt_type: sprint
sprint: AT.1
branch: feature/pat-s1-install-and-publish
base: origin/integrate/phase-at @ 2bad2812a
status: install-parity-complete
testpypi: retargeted-to-first-kit-era-tag-v1.4.4-at-phase-close
production: pending-contemporaneous-authorization
```

## Immutable inputs

| Item | Value |
| --- | --- |
| `sc-publish` revision | `42e0fcea23f730fae0ef3d08b060cd4df6a2602e` |
| Package file count | 49 |
| Package digest | `75da9d18426eacb92bab3e02bb6655a35e14f69deafe1ba2d00f4e93aabc0a5b` |
| Package-digest algorithm | SHA-256 over the newline-joined, path-sorted `"<sha256(file)>  <relative-path>"` lines from `install.package_files()`; source-relative paths, excluding the package source-root marker, `__pycache__`, and `*.pyc`. |
| Consumer input SHA-256 | `dfa4e053425a42714e957d4cf35118e11545dec21eed38a22e58b96c8c80c017` |
| `release/publish-artifacts.toml` SHA-256 | `f6d5ba976b01c255c14e35fe93322bc5a082fa6dc3989546e5867007c5d37287` |
| `release/publish-channel-contracts.toml` SHA-256 | `88d7ac055689c1e9fcd752c178eac9094ca29084a097fa7aff109c4fb0f40fd8` |

The local `/Users/randlee/Documents/github/sc-publish` checkout had the right
`develop` SHA but an untracked `.sc-compose/`; it was left untouched. All
installer and verification commands instead used a clean detached scratch clone
at the recorded SHA. The installed consumer-root `install.py` was never run.

## Live workspace census

`cargo metadata --no-deps --format-version 1` recorded workspace version
`1.4.4` and 22 workspace members. Every member received an explicit
disposition in the consumer input: 13 crates publish and 9 remain internal
`publish = false` crates. `peer-tls` is publishable because the publishable
`atm-daemon-bootstrap` has it as a normal dependency; it is order 3, after
`atm-storage` and before `atm-daemon-bootstrap`. No other live-census drift
from the declared consumer surface was found.

## Commands and validation

| Command | Exit | Result |
| --- | ---: | --- |
| `cargo metadata --no-deps --format-version 1` | 0 | Captured the 22-member, version-1.4.4 census and `peer-tls` dependency edge. |
| `git -C "$SP" rev-parse HEAD && git -C "$SP" status --porcelain` | 0 | Clean detached upstream clone at the required pin. |
| `python3 "$SP/plugins/sc-publish/.github/scripts/bootstrap_sc_compose.py" --venv .venv-sc-publish` | 0 | Installed the prebuilt `sc-compose==1.4.1`; no Cargo build or install. |
| `.venv-sc-publish/bin/python "$SP/plugins/sc-publish/install.py" --input release/sc-publish-consumer-input.json .` | 0 | Installed 49 shared files and rendered the two consumer manifests. |
| `.venv-sc-publish/bin/python "$SP/plugins/sc-publish/install.py" --dry-run --input release/sc-publish-consumer-input.json .` | 0 | `Publish-kit assets are in sync.` |
| `install.package_files()` parity sweep with `RENAMED_FILES` | 0 | `byte-identical: 49, mismatched: 0, missing: 0`. |
| `python3 .github/scripts/release_artifacts.py validate-manifest --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml` | 0 | Manifest validation passed. |
| `python3 .github/scripts/release_artifacts.py validate-publish-order --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml` | 0 | Publish order matches the workspace dependency graph. |
| `python3 .just/check_version_sync.py` | 0 | Workspace version, released Python artifacts, path dependencies, lockfile, Winget, and kit release wiring are aligned at 1.4.4. |
| `python3 .just/tests/test_release_preflight.py` | 0 | 3 passed: manifest/order, channel result, and candidate-provenance contract checks. |
| `python3 .just/tests/test_release_homebrew_workflow.py` | 0 | 3 passed: manifest config, declared formulas, credential/published-renderer contract checks. |
| `.venv-sc-publish/bin/python -m pytest -p no:cacheprovider "$SP/plugins/sc-publish/.github/scripts/tests/" -q` | 0 | `79 passed, 10 skipped, 3 subtests passed`; the scratch package checkout remained clean. |
| `just bootstrap` | 1 | Environment finding: `cargo-shear 1.13.3` requires Rust 1.95+, while the checked-in bootstrap contract selects Rust 1.94.1. This is a pre-existing bootstrap-pin incompatibility, not an AT.1 install/parity or package-byte defect. |
| `PATH="$PWD/.bootstrap-venv/bin:/tmp/at1-cargo-tools-20260827/bin:$PATH" just lint` | 0 | All recorded lint lanes exited 0; the temporary exact `cargo-shear 1.13.3` executable was built outside the repository only to complete validation after the bootstrap pin incompatibility. |
| `PATH="$PWD/.bootstrap-venv/bin:/tmp/at1-cargo-tools-20260827/bin:$PATH" just test` | 0 | 714 tests passed, 9 skipped. |

The package test dependency (`pytest==9.1.1`) was added only to the disposable
verification venv. The installed shared files were not locally edited; the
only expectation changes are ATM-owned lint/test data under `.just/`, as
required by ADR-050 and the AT.1 plan.

The bootstrap incompatibility is deliberately not patched in this sprint: it
is a repository-wide toolchain-pin decision outside AT.1's consumer-install
scope. It does not alter the pinned shared package, consumer input, rendered
manifests, or the successful lint/test evidence above.

## Channel and required-check evidence

The pinned rendered channel contract declares all required destinations:
crates.io, GitHub Releases, PyPI/TestPyPI, Homebrew, Winget, and Scoop.

| Requirement | Observed configured state |
| --- | --- |
| Repository secret `CARGO_REGISTRY_TOKEN` | Present (name-only API evidence) |
| Repository secret `HOMEBREW_TAP_TOKEN` | Present |
| Repository secret `WINGET_GITHUB_TOKEN` | Present |
| Repository secret `SCOOP_BUCKET_TOKEN` | Present |
| Environment `crates-io` | Present; no environment secret is declared by the contract |
| Environment `pypi` / `PYPI_API_TOKEN` | Present |
| Environment `testpypi` / `TEST_PYPI_API_TOKEN` | Present |

The GitHub branch-protection required-status-checks endpoint returned HTTP 404
for both `integrate/phase-at` and `develop`: neither branch currently has a
configured required-check list to migrate. Installed kit workflow job IDs are
`preflight`, `establish-provenance`, `gate-and-tag`, `release-plan`, `build`,
`build-python-wheels`, `build-python-sdists`, `verify-release`, `publish`,
`publish-crates`, `update-tap`, `publish-winget`, and `update-bucket`.
Transition plan: before either branch later gains required checks, select only
stable current CI checks (for example `just-lint`, `test`, `clippy`, and
`fmt`) plus any intentional release-gate job, and verify those exact job names
exist on the protected branch; do not retain names from the overwritten legacy
release workflow.

## Immutable 1.4.3 publication readiness

The published, non-draft release is
[`v1.4.3`](https://github.com/randlee/atm-core/releases/tag/v1.4.3) (release
ID `371881933`) with its four platform archives, checksums, published manifest,
surface scope, and release notes. Its tag commit
`fa6066468f8e638893bedd686244cffa2a43dbdf` is an ancestor of current
`origin/main` (`14eaf118dbc2615387949ad35069308f8cd870ea`). The package pin's
candidate gate applies before release creation; `pypi-publish.yml` is a
post-release retry workflow that accepts only `tag` and `target`, verifies the
non-draft release and attached assets, and neither rebuilds assets nor creates
tags.

## TestPyPI attempt evidence

Rand's verbatim authorization, relayed by Fenix, was: `"sure, testpypi"`.
Under that authorization, the post-release retry workflow was dispatched on
`integrate/phase-at` for tag `v1.4.3` and the TestPyPI target:

| Field | Value |
| --- | --- |
| Workflow run | [33040465059](https://github.com/randlee/atm-core/actions/runs/33040465059) |
| Workflow | `pypi-publish.yml` |
| Ref / inputs | `integrate/phase-at`; `tag=v1.4.3`; `target=testpypi` |
| Conclusion | Failed in `verify-release`; `publish` was skipped |
| TestPyPI effect | No package was uploaded |
| Production effect | None; production was neither authorized nor dispatched |

The failure is an upstream orchestration defect, not a release-artifact or
credential failure. The workflow checked out the historical release tag before
calling the repository-local `verify-published-release` action; `v1.4.3`
predates that action, so GitHub could not find its `action.yml`.

**Amendment (2026-08-27, owner decision — forward-only publishing.)** Rand
ruled: "I don't see a reason to try to re-publish anything before the first
kit install." Publishing tags that predate the kit installation is out of
scope, so the previously planned upstream repair was withdrawn (`sc-publish`
PR #57 closed unmerged; the policy is recorded there and as a one-line
limitation note in the `sc-publish` docs). No kit change is required: every
tag cut from a kit-installed tree carries the actions/scripts and the kit
manifest schema the workflows need. Independent verification during triage:
the `v1.4.3` tag tree also fails the current kit's manifest schema
(`channel-config` rejects it — `[project]` missing required keys — and
`publish-channel-contracts.toml` is absent at the tag), so `v1.4.3` is
unpublishable via the kit on two independent grounds. The TestPyPI leg is
therefore retargeted from `v1.4.3` to the first kit-era tag (workspace version
`1.4.4`), to be cut after phase AT merges to `develop`, with the publish leg
run as phase-close verification. Rand's TestPyPI authorization (`"sure,
testpypi"`) carries over to that retargeted dispatch. Production remains
`pending-contemporaneous-authorization`; no standing or advance authorization
will be treated as production permission.

## Findings and scope

No GENUINE STOP occurred. The temporary local upstream checkout with
`.sc-compose/` was bypassed in favor of a clean scratch clone. The absence of
branch protection is recorded as transition evidence, not a release blocker.
No legacy workflow or publish asset was deleted; AT.2 remains the only owner
of deletion decisions.

## Fix rounds AT1-FIX-R1 and AT1-FIX-R2

The R1 follow-up corrected consumer-owned validation without editing the
pinned kit or removing a legacy asset:

- **AT1-QA-001:** `scripts/validate_release.py` invokes the installed
  `.github/scripts/release_artifacts.py` contract for manifest, preflight,
  publish-plan, binary, and inventory operations. The retained root
  `scripts/release_artifacts.py` is not deleted and is used only by its
  legacy installed-doc compatibility path.
- **AT1-QA-002:** the AT.1 plan remains `in-progress` while TestPyPI and
  production publication are authorization-gated.
- **AT1-QA-003:** `.just/check_version_sync.py` delegates release-artifact
  version lockstep to the installed kit; the consumer linter retains only its
  local workspace-member policy.
- **AT1-QA-004:** real Cargo package and publish dry-runs still run locally.
  A failed dry-run is an Expected/Proceed warning only when Cargo reports all
  of: an internal workspace package, an exact `^1.4.4` requirement, candidate
  versions that do not match, and the public crates.io index. Compilation,
  packaging, malformed-manifest, and third-party dependency failures remain
  blocking. This implements the phase README's not-yet-published-version row;
  it is not a blanket dry-run bypass.

| Command | Exit | Result |
| --- | ---: | --- |
| `just validate` | 0 | Default pre-publish validation passed. The current coordinated release version produced only the documented, shape-specific unpublished-internal-version dry-run warnings; no blockers. |
| `just lint` | 0 | Full lint recipe passed. |
| `just test` | 0 | `708` tests passed, `9` skipped. This is six fewer than the earlier 714-test AT.1 run because the kit-delegation change replaced ten root `check_version_sync` behavior tests with four installed-kit delegation tests. |
| `python3 .just/tests/test_validate_release.py` | 0 | `9` tests passed, including the R2 expected-warning and non-workspace blocking cases. |
| `.venv-sc-publish/bin/python -m pytest -p no:cacheprovider "$SP/plugins/sc-publish/.github/scripts/tests" -q` | 0 | `79 passed, 10 skipped, 3 subtests passed` against clean detached pin `42e0fcea23f730fae0ef3d08b060cd4df6a2602e`. |
| `.venv-sc-publish/bin/python "$SP/plugins/sc-publish/install.py" --dry-run --input release/sc-publish-consumer-input.json .` | 0 | `Publish-kit assets are in sync.` |
| `install.package_files()` parity sweep with `RENAMED_FILES` | 0 | `byte-identical: 49, mismatched: 0, missing: 0`. |

The TestPyPI attempt above is the only publish dispatch in the AT.1 work. It
failed before publication and did not upload an artifact. Production remains
authorization-gated and was never dispatched. The clean detached scratch
checkout was used for every pin-sensitive command; the developer checkout with
its untracked `.sc-compose/` directory was not modified.
