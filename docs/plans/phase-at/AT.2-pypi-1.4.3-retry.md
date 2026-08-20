# AT.2 — 1.4.3 TestPyPI And PyPI Retry

```yaml
plan_type: sprint_plan
phase: AT
sprint: AT.2
worktree: feature/pat-s2-pypi-143-retry
branch: feature/pat-s2-pypi-143-retry
execution_target: immutable main release assets (workflow dispatch by tag)
status: proposed
estimated_scope: one Python release rehearsal followed by one authorized production retry
```

## Goal

Prove the canonical shared workflow by publishing every manifest-declared
Python distribution for immutable ATM `main` version `1.4.3` to TestPyPI and,
after explicit authorization, PyPI. This is a retry of release assets that
were not published by the earlier workflow, not a source change or version
bump.

## Branch And Review Mechanics

Workflow dispatches run against the immutable `main` tag and its release
assets and change no source. All sprint artifacts — receipts and doc-status
updates — land on `feature/pat-s2-pypi-143-retry` and merge through a normal
PR under the quality-mgr gate (phase README Branch Strategy). The sprint
never commits to `main`.

**Dispatch mechanics.** `gh workflow run` executes the workflow YAML from the
ref given to `--ref`, not from the release tag. AT.2 therefore dispatches
with `--ref integrate/phase-at` — the branch where AT.1's installed kit
workflows live after the AT.1 PR merges — while the immutable release
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

## Deliverables

1. Resolve the immutable `main` commit, version, signed/tagged release assets,
   AT.1 package revision, consumer-input digest, manifest digest, and build
   receipt before any upload. Reject an asset/version/digest mismatch.
2. Before the first TestPyPI/PyPI dispatch, verify the `hermes-atm` and
   `atm-graft` PyPI descriptions/READMEs and the GitHub release notes carry
   the first-public-release framing decided in
   [ADR-049](../../adr/ADR-049-hermes-atm-first-public-pypi-release-versioning.md);
   record the check in the receipt.
3. With the release authorizer's TestPyPI authorization on record (it may be
   granted in advance in the sprint dispatch message; quote it verbatim in
   the receipt), execute the canonical release workflow against that
   immutable source for TestPyPI. It must build/select all manifest-declared
   wheels and sdists, validate them, and perform a real retry-safe upload
   with `--skip-existing` behavior.
4. Verify TestPyPI's public package/version/file set and install each package
   from the public index on Python 3.11, 3.12, 3.13, and 3.14.
5. After contemporaneous production authorization from the release authorizer
   (quoted verbatim in the receipt; advance or standing authorization is not
   valid for production), repeat the same immutable asset and validation path
   for PyPI. Store public URLs, hashes, workflow run IDs, interpreter
   results, and the package/tool/manifest identity tuple.

## Prerequisites

- AT.1 acceptance criteria pass for the exact package revision and input, and
  the AT.1 PR is merged to `integrate/phase-at` (the dispatch `--ref`
  target).
- `main` contains the intended immutable 1.4.3 source and release assets.
- The release authorizer explicitly authorizes the TestPyPI and PyPI
  dispatches. The release authorizer is defined in the phase README: only
  the human repository owner (Rand), acting outside any agent role — never
  an agent, coordinator, or teammate. Each authorization is quoted verbatim
  in the receipt. TestPyPI authorization may be granted in advance in the
  sprint dispatch message; production authorization must be contemporaneous
  with the production dispatch.

## Dependencies

- `AT.1`: `must_follow`; no replacement package revision, input, manifest, or
  tool bootstrap may be substituted after its proof.
- Channel actions are ordered: TestPyPI public verification must pass before
  any PyPI dispatch.
- `AT.3`: this sprint's receipts are the coverage evidence AT.3 requires
  before deleting legacy paths. The TestPyPI receipt is AT.3's minimum gate;
  the hermes legacy-workflow rows (the workflow, its feeder script, and
  their tests) additionally require the production PyPI receipt (see
  TestPyPI Checkpoint below and AT.3's Deletion Candidates `Gate` column).

## TestPyPI Checkpoint

TestPyPI success plus its receipt section is an independently mergeable,
QA-checkable checkpoint. If production authorization is delayed, the sprint
PR may merge with the TestPyPI receipt and an explicit
`production: pending-authorization` marker; the PyPI dispatch then lands as
a follow-up commit/PR on the same receipt file
(`docs/plans/phase-at/receipts/AT.2-receipt.md`) without reopening AT.1.

## Non-Goals

- No product-source changes (sprint artifacts on the feature branch are
  receipts and doc-status updates only), no version bump, crate publication,
  GitHub Release rebuild, Homebrew, Scoop, or Winget publication, and no
  Hermes gateway/product onboarding flow. The `hermes-atm` public-index
  install/import check (pip install from TestPyPI/PyPI and module import on
  each supported interpreter) IS required — it is part of this sprint's
  verification, not Hermes onboarding.
- No token rotation, token-value logging, credential workaround, or claim that
  a GitHub-environment credential exists without workflow evidence.

## Paths To Delete

None. This is publication evidence only; it deletes or replaces no workflow,
asset, tag, or release.

## Acceptance Criteria

- TestPyPI and PyPI contain every Python distribution declared in the same
  immutable manifest, with exactly version 1.4.3 and matching hashes.
- Each supported Python version installs from the public index and imports the
  declared module without using local wheels, paths, or a source checkout.
- The workflow records a structured, channel-specific failure if a GitHub
  environment credential is unavailable. A channel-scoped
  credential-unavailable (fail-closed) result is valid evidence the workflow
  fails closed, and per the phase README Non-Blocking Outcomes table it is
  never an emergency — but it does NOT constitute publication: it does not
  close this sprint's first acceptance criterion and does not satisfy AT.3's
  coverage-evidence prerequisite.
- The PyPI descriptions and release notes carry ADR-049's
  first-public-release framing, verified before the first dispatch
  (Deliverable 2).
- Receipts prove the same source commit, package revision, consumer-input
  digest, manifest digest, tool bootstrap, asset hashes, and validation
  results for TestPyPI and PyPI.

## Required Validation

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

## Required Document Updates

Append the sprint receipt at `docs/plans/phase-at/receipts/AT.2-receipt.md`
(shape per the phase README's Receipt Convention) containing the
TestPyPI/PyPI URLs, workflow run IDs, source and asset identities,
index-install matrix, and any channel-scoped fail-closed credential result.

## Risks And Watchouts

- Public indexes are immutable: an existing different file/version is a hard
  stop; do not overwrite or rebuild under the same version.
- A retry only reuses the same verified immutable assets. Any changed asset or
  manifest starts a new release decision.
- Classify every non-passing result against the phase README's Non-Blocking
  Outcomes table before reacting; only its GENUINE STOP row halts work.
