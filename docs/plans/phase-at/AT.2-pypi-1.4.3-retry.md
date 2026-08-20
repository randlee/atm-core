# AT.2 — 1.4.3 TestPyPI And PyPI Retry

```yaml
plan_type: sprint_plan
phase: AT
sprint: AT.2
worktree: main immutable release assets
branch: main
status: proposed
estimated_scope: one Python release rehearsal followed by one authorized production retry
```

## Goal

Prove the canonical shared workflow by publishing every manifest-declared
Python distribution for immutable ATM `main` version `1.4.3` to TestPyPI and,
after explicit authorization, PyPI. This is a retry of release assets that
were not published by the earlier workflow, not a source change or version
bump.

## Deliverables

1. Resolve the immutable `main` commit, version, signed/tagged release assets,
   AT.1 package revision, consumer-input digest, manifest digest, and build
   receipt before any upload. Reject an asset/version/digest mismatch.
2. Execute the canonical release workflow against that immutable source for
   TestPyPI. It must build/select all manifest-declared wheels and sdists,
   validate them, and perform a real retry-safe upload with `--skip-existing`
   behavior.
3. Verify TestPyPI's public package/version/file set and install each package
   from the public index on Python 3.11, 3.12, 3.13, and 3.14.
4. After explicit production authorization, repeat the same immutable asset
   and validation path for PyPI. Store public URLs, hashes, workflow run IDs,
   interpreter results, and the package/tool/manifest identity tuple.

## Prerequisites

- AT.1 acceptance criteria pass for the exact package revision and input.
- `main` contains the intended immutable 1.4.3 source and release assets.
- The operator explicitly authorizes the TestPyPI and PyPI dispatches.

## Dependencies

- `AT.1`: `must_follow`; no replacement package revision, input, manifest, or
  tool bootstrap may be substituted after its proof.
- Channel actions are ordered: TestPyPI public verification must pass before
  any PyPI dispatch.
- `AT.3`: this sprint's successful run is the coverage evidence AT.3 requires
  before deleting legacy paths.

## Non-Goals

- No source changes, version bump, crate publication, GitHub Release rebuild,
  Homebrew, Scoop, Winget, or Hermes installation.
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
  environment credential is unavailable; that is valid fail-closed evidence,
  not an unbounded release or planning blocker.
- Receipts prove the same source commit, package revision, consumer-input
  digest, manifest digest, tool bootstrap, asset hashes, and validation
  results for TestPyPI and PyPI.

## Required Validation

```bash
python3 .github/scripts/release_artifacts.py validate-manifest --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml
python3 .github/scripts/release_artifacts.py verify-python-release-assets --manifest release/publish-artifacts.toml --asset-dir <immutable-release-assets>
python3 -m pip install --index-url https://test.pypi.org/simple --only-binary=:all: <package>==1.4.3
python3 -m pip install --index-url https://pypi.org/simple --only-binary=:all: <package>==1.4.3
```

Run the public-index install commands in fresh Python 3.11–3.14 environments
for every manifest-declared Python package.

## Required Document Updates

Append one immutable release receipt containing the TestPyPI/PyPI URLs,
workflow run IDs, source and asset identities, index-install matrix, and any
channel-scoped fail-closed credential result.

## Risks And Watchouts

- Public indexes are immutable: an existing different file/version is a hard
  stop; do not overwrite or rebuild under the same version.
- A retry only reuses the same verified immutable assets. Any changed asset or
  manifest starts a new release decision.
