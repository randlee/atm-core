# Publish Kit Requirements

> Document role: Normative requirements for the manifest-driven, vendorable
> release/publish kit. This is workflow/tooling scope, not a record of any
> specific release execution.

## 1. Manifest-Driven Publish Kit

- Every repo-specific release deliverable (crates, binaries, targets,
  Python distributions, channel destinations) is declared in the manifest
  (`release/publish-artifacts.toml`), not hardcoded in workflow YAML or
  agent prompts.
- Adopting the kit in a consumer repo is a manifest edit only — no
  workflow or code changes required.

## 2. Parallel Per-Channel Orchestration

- The named `publisher` fans out one role-specific background worker/job per
  publish channel (crates.io, GitHub Release, Homebrew, `winget`, Scoop, PyPI)
  running in parallel inside its session,
  not sequentially.
- Each background channel worker consolidates and owns exactly what its specific
  target needs (its own manifest-declared inputs, its own publish steps,
  its own verification), rather than one monolithic publish step handling
  every channel.
- Structured per-channel results are collected centrally.

### 2.1 Homebrew Formula Tracks and Executables

- Each `[[channels.homebrew.formulas]]` entry declares its destination path,
  renderer template, Ruby class, `binaries`, test fields, and
  `release_track = "stable" | "prerelease"` in
  `release/publish-artifacts.toml`.
- A stable tag renders, validates, and commits every `stable` formula entry;
  a prerelease tag does the same only for `prerelease` entries. Formula names,
  paths, templates, and classes never appear as workflow literals.
- `binaries` is the canonical non-empty list of archive binaries installed by
  the formula. `test_binary` defaults to its first entry and must name an
  entry in that list. Legacy `binary` manifests normalize to a one-entry list
  for vendor compatibility; newly authored manifests use `binaries`.

## 3. Independent Per-Channel Retry

- All publish channels can be independently retried.
- A failure in one channel (e.g. Scoop) does not require re-running
  channels that already succeeded (e.g. crates.io, Homebrew).
- Retry is scoped to the failed channel(s) only, using the structured
  per-channel results from requirement 2.

## 4. Non-Disclosing Credential Preflight

- A mandatory preflight step runs before release dispatch and is the sole
  authority on credential liveness. It is fail-closed but not fail-fast: it
  records every independent check before returning one final authorization
  verdict; only checks whose prerequisites failed may be marked `blocked`.
- The preflight never inspects, exposes, or prints a secret value. It
  establishes liveness via non-disclosing checks:
  - GitHub-destination tokens (`HOMEBREW_TAP_TOKEN`, `WINGET_GITHUB_TOKEN`,
    `SCOOP_BUCKET_TOKEN`, `CARGO_REGISTRY_TOKEN`): authenticate against the
    GitHub/target API to detect revoked or expired tokens.
  - PyPI/TestPyPI tokens (`PYPI_API_TOKEN`, `TEST_PYPI_API_TOKEN`,
    environment-scoped): inspect environment-secret *metadata* (e.g.
    existence, secret name) without binding the preflight job to the
    approval-gated `pypi`/`testpypi` environments.
  - Where token liveness cannot be established by metadata alone, define a
    safe, channel-specific rehearsal/health check instead of skipping the
    check.
- Tests must cover missing/rejected-token diagnostics without ever
  asserting on or logging a secret value.

## 5. Agent Behavior Around Credentials

- The publisher (and any channel subagent) MUST NOT ask whether a token
  exists, request a token, ask to re-enter a token, or inspect/expose a
  token value, under normal operation.
- The **only** exception: if the non-disclosing credential preflight
  (requirement 4) actually fails for a given channel, the agent reports
  that specific failure (channel + non-disclosing diagnostic) to
  `team-lead`. It still does not ask the user or comp for the secret
  value itself — reporting the failure is the extent of the escalation.
- All release secrets use the same GitHub Actions secret names across
  every repo that vendors this kit; secret names are fixed by the shared
  channel contract, never per-repository.
- Every root or post-release channel worker receives both its manifest-derived
  preflight contract and its completed, non-disclosing preflight result before
  it may publish or retry. A worker denies only its own channel when that
  evidence is missing, failed, stale, or mismatched; it must not restart other
  channels or ask for credentials.
- Repository-secret and credential-liveness outcomes are keyed by channel in
  the preflight result. The workflow may retain an aggregate failure to deny
  the overall release, but it must not copy that aggregate failure into
  unrelated channel results.

## 5.1 Shared Channel Contract

- `release/publish-channel-contracts.toml` is vendored unchanged with the
  publish kit and is the sole machine-readable source for channel identity,
  standardized secret names, GitHub environments, public registry endpoints,
  liveness checks, and role-specific background-worker contracts.
- The artifact manifest contains only repository-specific artifacts and
  destinations. It must not repeat credential or account protocol.
- Release Preflight checks public registry state for every declared crate and
  Python distribution. An absent project is reported as an available new name;
  a public lookup is never a name reservation. Existing production versions
  fail closed; TestPyPI state is rehearsal information.

## 6. Scope Boundary

- This kit provides the workflow/tooling only.
- Installing or upgrading the kit does not dispatch, tag, or publish any
  actual release. Publishing a real release is a separate decision and
  requires explicit sign-off from the consuming repository's release owner.
