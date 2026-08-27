# AT.2 legacy publish-surface deletion receipt

```yaml
receipt_type: sprint
sprint: AT.2
branch: feature/pat-s2-legacy-publish-deletion
worktree: feature/pat-s2-legacy-publish-deletion
status: complete
kit_revision: 42e0fcea23f730fae0ef3d08b060cd4df6a2602e
```

## Immutable inputs

| Item | Value |
| --- | --- |
| `sc-publish` revision | `42e0fcea23f730fae0ef3d08b060cd4df6a2602e` |
| Installable package files | 49 |
| Package digest | `75da9d18426eacb92bab3e02bb6655a35e14f69deafe1ba2d00f4e93aabc0a5b` |
| Package-digest algorithm | SHA-256 over the newline-joined, path-sorted `"<sha256(file)>  <relative-path>"` lines for `install.package_files()`, excluding `.sc-publish-source-root`, `__pycache__`, `*.pyc`, and pre-rendered `release/publish-*.toml`. |
| Consumer-input SHA-256 | `ba518f7f1d10fc25d788d15bf68398483fd1691b9604dabbb83196d74b23d2c2` |
| `release/publish-artifacts.toml` SHA-256 | `f6d5ba976b01c255c14e35fe93322bc5a082fa6dc3989546e5867007c5d37287` |
| `release/publish-channel-contracts.toml` SHA-256 | `88d7ac055689c1e9fcd752c178eac9094ca29084a097fa7aff109c4fb0f40fd8` |

## Gate state

AT.2 follows the forward-only ruling: no pre-kit tag is republished. The
first kit-era `1.4.4` release has not yet produced its crates.io, GitHub
Release, channel, TestPyPI, or production-PyPI receipts. Therefore every
table row whose gate is a future publication receipt is retained as an
explicit deferral; this is an AT.2 completion outcome, not a failed deletion.

The independent GitHub Actions history command was:

```text
gh run list --workflow=pypi-publish.yml --limit 30 --json databaseId,displayTitle,event,headBranch,headSha,status,conclusion,url,createdAt
[{"conclusion":"failure","createdAt":"2026-08-27T04:47:15Z","databaseId":33040465059,"displayTitle":"Publish PyPI","event":"workflow_dispatch","headBranch":"integrate/phase-at","headSha":"e8722dff6637724767504915963015055af28553","status":"completed","url":"https://github.com/randlee/atm-core/actions/runs/33040465059"}]
```

This establishes that run `33040465059` is the only TestPyPI/PyPI retry
dispatch. It failed before its publish job, so no package was uploaded and no
production dispatch occurred.

## Candidate dispositions

The original prescribed scan covered `.github/`, `.just/`, `Justfile`,
`scripts/`, and `docs/`; AT2-FIX-R2 widened it to the whole tracked repository,
including `release/`. The wider audit found the stale active publish-surface
prose and removed it. It excludes only immutable historical plans, receipts,
and triage evidence when classifying a live caller. `AT.2 plan` means the
remaining mention is this disposition table; `historical receipt` means
immutable evidence rather than a live caller.

| Path | Disposition | Gate / coverage statement | Reference-scan result |
| --- | --- | --- | --- |
| `.github/workflows/hermes-atm-pypi-publish.yml` | `deferred-until-production-pypi-receipt` | Kit `pypi-publish.yml` must first prove all declared distributions, including `hermes-atm`, on production PyPI. | AT.2 plan only. |
| `scripts/prepare_hermes_atm_publish_artifacts.py` | `deferred-until-production-pypi-receipt` | Same production kit leg; this script feeds the legacy workflow. | Legacy workflow plus AT.2 plan. |
| `.just/tests/test_hermes_atm_pypi_publish_workflow.py` | `deferred-until-production-pypi-receipt` | Temporary compatibility coverage until the production receipt exists. | AT.2 plan only. |
| `.just/tests/test_prepare_hermes_atm_publish_artifacts.py` | `deferred-until-production-pypi-receipt` | Temporary compatibility coverage until the production receipt exists. | AT.2 plan only. |
| `scripts/release_artifacts.py` | `deferred-until-first-kit-release-receipt` | Root legacy helper remains until the first kit release proves `.github/scripts/release_artifacts.py` end-to-end. | Live references were removed from retained consumers; remaining references are historical/kit paths and AT.2 planning. |
| `scripts/release_gate.sh` | `deferred-until-first-kit-release-receipt` | Kit `.github/scripts/release_gate.sh` in `release.yml` needs first-release evidence. | Retained validator and installed workflow references; no deletion before its gate. |
| `scripts/verify_release_archive.py` | `retained-atm-owned-data-deferred-until-first-kit-release-receipt` | It now reads the consumer manifest's `release_binaries[].bundled_paths` directly, asserting ATM archive membership data while the kit owns generic asset verification. | AT.2 plan and project-plan references only. |
| `scripts/validate_release.py` | `retained-atm-owned-data-deferred-until-first-kit-release-receipt` | Live `just validate` consumer policy: workspace dry-run classification, lint/CLI/inventory orchestration, and findings output remain ATM-owned. Installed-kit manifest/preflight commands are delegated. | `Justfile`, smoke/preflight docs, and the retained test. |
| `.just/tests/test_release_artifacts.py` | `deferred-until-first-kit-release-receipt` | Temporary legacy compatibility coverage; its module docstring records the deferral. | AT.2 plan plus historical rehearsal evidence. |
| `.just/tests/test_release_gate.py` | `deferred-until-first-kit-release-receipt` | Temporary legacy compatibility coverage; its module docstring records the deferral. | AT.2 plan only. |
| `.just/tests/test_release_homebrew_workflow.py` | `retained-atm-owned-data-deferred-until-homebrew-channel-receipt` | Asserts consumer manifest formula data rendered by the kit workflow. | AT.2/AT.1 plans and historical receipt evidence. |
| `.just/tests/test_release_preflight.py` | `deferred-until-first-kit-release-receipt` | Temporary consumer-contract coverage pending the first kit release. | AT.2/AT.1 plans and historical receipt evidence. |
| `.just/tests/test_validate_release.py` | `retained-atm-owned-data-deferred-until-first-kit-release-receipt` | Tests the retained consumer validator and its kit delegation boundary. | Smoke documentation, AT.1 receipt, and AT.2 plan. |
| `.winget/randlee.agent-team-mail.yaml` | `deferred-until-winget-channel-receipt` | Kit `winget-publish.yml` must first prove generated-manifest publication. | ATM version-sync test and AT.2 plan. |

## Structural legacy removals and boundary corrections

| Item | Disposition | Evidence |
| --- | --- | --- |
| Legacy installed-document manifest branch | deleted | The obsolete consumer-input table and `scripts/release_artifacts.py` / `validate_release.py` parsing, staging, listing, and validation paths are gone. The rendered manifest expresses document shipping through `release_binaries[].bundled_paths`; regression tests cover a legacy table as inert input rather than restoring execution. |
| Retired user-document verifier and its unit-test module | deleted | They only supported the retired unreachable branch. The default test runner no longer invokes them; the widened whole-repository audit removed stale current publish-surface prose. Historical Phase AE evidence is retained without presenting a live path. |
| Root `release_artifacts` imports | removed | `validate_release.py` no longer imports the root helper. `verify_release_archive.py` now parses consumer-owned `bundled_paths` directly; directory, file, Windows, and missing-source cases are covered. |
| Phase AD readiness | sunset from `all`/`validate` | The explicit `phase-ad-readiness` target remains only for the thorough-smoke lane; a unit test proves it is not in `DEFAULT_RELEASE_TARGETS`. |
| Workspace-member enumeration | deferred with root helper | `scripts/release_artifacts.py` cannot be deleted before its first-kit-release receipt, so its fourth enumeration remains temporarily. AT.2 adds no shared helper and records the remaining duplication rather than masking it. |

## Validation evidence

| Command | Exit | Result |
| --- | ---: | --- |
| Pinned checkout bootstrap + `install.py --dry-run` | 0 | `sc-compose==1.4.1` bootstrapped in `.venv-sc-publish-at2`; detached pin `42e0fcea23f730fae0ef3d08b060cd4df6a2602e` reported `Publish-kit assets are in sync.` |
| `python3 .just/tests/test_validate_release.py` | 0 | 7 tests passed, including the legacy-schema non-execution regression and Phase AD default-target sunset. |
| `python3 .just/tests/test_verify_release_archive.py` | 0 | 2 tests passed: directory/file bundle expansion, Windows binary naming, and fail-closed missing-source coverage. |
| `just lint` | 0 | Full repository lint gate passed. |
| `just test` | 0 | 703 tests passed, 9 skipped. |
| Workflow YAML parse | 0 | All `.github/workflows/*.yml` parsed successfully. |
| Deleted-path executable scan | 0 | No executable reference remains to either retired user-document verifier path; the AT.2 receipt is the sole disposition record. |
| Whole-repository legacy-branch audit | 0 | No live release-validation, staging, or publish-surface reference remains to the retired user-document verifier or installed-document manifest branch. Historical plans/receipts/triage and unrelated product installed-document APIs were classified separately. |

## AT2-FIX-R2 findings

- **ATM-QA-009:** Widened the deletion-reference scan to include `release/` and
  replaced the stale user-document freshness/staging prose with the current
  `release_binaries[].bundled_paths` shipping contract.
- **ATM-QA-010:** Added the module-level ATM-owned-data retention rationale to
  `.just/tests/test_validate_release.py`.
- **ATM-QA-011:** Recorded the pinned package identity, consumer-input hash,
  and both rendered-manifest hashes above using the phase Receipt Convention.
