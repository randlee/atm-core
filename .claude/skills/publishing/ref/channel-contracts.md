# Publish Channel Contracts

`release/publish-channel-contracts.toml` is the sole channel-contract source.
It defines names, standard secret names, GitHub environments, public endpoints,
and liveness checks. This document defines only the operating procedure for
using that contract. Copy both files unchanged when vendoring the publish kit.

## Common rules

- Tokens are GitHub Actions secrets with the names declared in the TOML. Never
  request, inspect, print, or replace one locally.
- Credential facts:
  - `PYPI_API_TOKEN` — Actions `pypi` environment; preflight:
    `.github/workflows/release-preflight.yml`; publish:
    `.github/workflows/pypi-publish.yml`.
  - `TEST_PYPI_API_TOKEN` — Actions `testpypi` environment; preflight:
    `.github/workflows/release-preflight.yml`; publish:
    `.github/workflows/pypi-publish.yml`.
  - `CARGO_REGISTRY_TOKEN` — repository secret for preflight:
    `.github/workflows/release-preflight.yml`; the publish job runs in the
    Actions `crates-io` environment in `.github/workflows/release.yml`.
  - `HOMEBREW_TAP_TOKEN` — repository secret; preflight:
    `.github/workflows/release-preflight.yml`; publish:
    `.github/workflows/homebrew-publish.yml`.
  - `WINGET_GITHUB_TOKEN` — repository secret; preflight:
    `.github/workflows/release-preflight.yml`; publish:
    `.github/workflows/winget-publish.yml`.
  - `SCOOP_BUCKET_TOKEN` — repository secret; preflight:
    `.github/workflows/release-preflight.yml`; publish:
    `.github/workflows/scoop-publish.yml`.
  - `GITHUB_TOKEN` — GitHub-provided Actions token; preflight runs with
    `contents:read` and verifies the release declaration in
    `.github/workflows/release-preflight.yml`; publish uses `contents:write`
    in `.github/workflows/release.yml`.
- These credentials are already configured. Do not ask whether they exist or
  ask anyone to provide them; run the named preflight workflow and report its
  sanitized result.
- A public lookup is evidence of registry state, not a reservation. Treat a
  timeout, rate limit, unexpected response, or 5xx as `indeterminate`.
- `publisher` may delegate a read-only inquiry to a role-specific background
  worker without a release assignment. Publishing, workflow dispatch, or retry
  still requires the `publisher` assignment and successful preflight evidence.

## crates.io and PyPI inquiry — `crates-io-publisher`, `pypi-publisher`

For a publisher-delegated, read-only candidate-name or candidate-version inquiry, generate
the contract-derived public URLs before calling `curl`:

```bash
python3 .github/scripts/release_artifacts.py public-registry-inquiry-plan \
  --contracts release/publish-channel-contracts.toml \
  --channel crates_io --name atm-serde --version 0.1.0
```

For each returned URL, issue a public `curl --silent --show-error --output
/dev/null --write-out '%{http_code}' <url>` request. A project `404` is
`apparently_available`; project `200` is `taken`; another status is
`indeterminate`. If a version was supplied, a version `200` is `already_live`,
while version `404` is available for a project that is already taken. A query
does not reserve a name. The helper applies PEP 503 normalization for PyPI and
reports TestPyPI as rehearsal information only.

For a partial crates.io retry, derive the full check plan from the manifest,
retain `already_live` entries, and publish only the missing set in manifest
order. Cargo publishing is permanent; never bump a version merely because a
new crate was missing in an earlier run.

## GitHub Release — `github-release-publisher`

No external secret is needed. The root release workflow must declare
`contents: write`; it owns tag and immutable GitHub Release creation. Never
create or move tags manually.

## Homebrew — `homebrew-publisher`

Use the contract-declared GitHub token liveness check before dispatching the
manifest-declared tap workflow. Destination repository, formulas, assets, and
verification commands come from the manifest. Each `[[channels.homebrew.formulas]]`
entry declares its path, template, class, `binaries`, test fields, and
`release_track`; stable tags select every `stable` entry, while prerelease tags
select only `prerelease` entries. `test_binary` defaults to the first binary.
For vendor compatibility, a legacy single `binary` normalizes to a one-entry
`binaries` list; new manifests must use `binaries`.

## winget — `winget-publisher`

Use the contract-declared GitHub token liveness check before dispatching the
manifest-declared workflow. The manifest owns the package identifier and
installer target.

## Scoop — `scoop-publisher`

Use the contract-declared GitHub token liveness check before dispatching the
manifest-declared workflow. The manifest owns the bucket, manifest path,
template, and installer target.
