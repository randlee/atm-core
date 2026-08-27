# AT.1 Rehearsal Receipt — Canonical Consumer Install

```yaml
doc_type: rehearsal_receipt
sprint: AT.1
branch: smoke/phase-at-at1-rehearsal
baseline: develop @ d610b4c07 (merge of PR #960)
date: 2026-08-20
verdict: clean-with-findings
```

This is a rehearsal of AT.1 (`docs/plans/phase-at/AT.1-canonical-consumer-install.md`
on `plan/phase-at-publish-recovery`), executed end-to-end on a smoke branch to
produce execution evidence. It is not the AT.1 sprint itself.

## Pinned package

| Item | Value |
| --- | --- |
| `sc-publish` revision | `0fa5b05e44a655ec76ada8a6c2b24714d47acca1` (develop, merge of sc-publish PR #45) |
| Package root | `plugins/sc-publish/` inside the pinned checkout |
| Package digest | `a0da19677d150e565e724d99a442f660974520d4f429b71da1b3f03a6bbc1a83` |
| Digest algorithm | sha256 over the newline-joined, path-sorted list of `"<sha256(file)>  <relative-path>"` lines for all 48 installable package files (excludes `.sc-publish-source-root`, `__pycache__`, `*.pyc`, and any pre-rendered `release/publish-*.toml`). The AT.1 doc requires "package digest" without defining an algorithm — see finding F2. |
| Installable file count | 48 |
| sc-compose bootstrap | `bootstrap_sc_compose.py` pinned `sc-compose==1.4.1`, installed into `.venv-sc-publish/` (uncommitted; already gitignored) |

## Consumer input

| Item | Value |
| --- | --- |
| File | `release/sc-publish-consumer-input.json` |
| sha256 | `7fbcb24b6afbacbc4d0259527cf851e255809fa1fbda8ae9233a13dcde24643b` |

Derived from the 2026-08 reconciliation-spike draft (validated against
`ce85b4d`), then re-verified field-by-field against the repo and the pinned
schema in `install.py`:

- **Crates**: all 12 publishable workspace crates (`cargo metadata` `publish=None`)
  are declared `publish=true` in publish order 1–12; `atm-graft-python` and
  `atm-query-python` (workspace `publish=false`, released to PyPI not
  crates.io) are declared `publish=false`/`publish_order=0`. Publish order was
  machine-checked against `cargo metadata`: no published crate depends
  (normal deps) on a later-ordered or unpublished workspace crate. Internal
  non-release crates (`atm-architecture`, `atm-peer-tls-interop`,
  `atm-runtime-test-support`, `atm-storage-sqlserver-proof`, `sc-lint-*`) are
  not declared, matching the legacy manifest's release surface — see finding F3.
- **Python distributions**: `atm-graft` (maturin via `cargo_manifest`),
  `atm-query` (maturin via `cargo_manifest`), `hermes-atm`
  (`build_system: "setuptools"`, matching `crates/hermes-atm/pyproject.toml`
  `build-backend = "setuptools.build_meta"`). All source/module/manifest paths
  verified to exist.
- **Binaries**: `atm` (with bundled `docs/user-documents`) and `atm-daemon`,
  matching the legacy manifest. The workspace also builds
  `atm_post_send_hook_fixture` (test fixture), `atm-daemon-benchmark`
  (benchmark tool), and `sc-lint-boundary` (internal lint bin); none are
  release binaries, consistent with the legacy surface.
- **Channels**: pypi (testpypi/pypi), homebrew (`randlee/homebrew-tap`, 3
  formulas each with `test_binary`), winget (`randlee.agent-team-mail`), scoop
  (`randlee/scoop-bucket`).

### Fields changed from the spike draft

| # | Change | Reason |
| --- | --- | --- |
| 1 | Added `project.workspace_toml = "Cargo.toml"` | AT.1 deliverable: input declares the version source explicitly; kit otherwise defaults it ("nothing is inferred"). Verified `Cargo.toml` declares `[workspace.package].version = "1.4.3"`. |
| 2 | Added `project.rust_toolchain = "1.94.1"` | Same explicitness principle; matches `rust-toolchain.toml` (`channel = "1.94.1"`). Kit default happens to be `1.94.1` too, but relying on the default is inference. |

No other field needed changing: the draft passed the pinned (`0fa5b05`)
installer schema on the first attempt. The `ce85b4d → 0fa5b05` schema delta did
not invalidate the draft (the `test_binary` addition from the spike is already
present and is required by the pinned schema).

## Install and parity evidence

All commands run from the worktree root
(`/Users/randlee/Documents/github/atm-core-worktrees/smoke/phase-at-at1-rehearsal`);
`$SP` is the pinned read-only package checkout.

| # | Command | Exit | Result |
| --- | --- | --- | --- |
| 1 | `python3 $SP/plugins/sc-publish/.github/scripts/bootstrap_sc_compose.py --venv .venv-sc-publish` | 0 | Installed `sc-compose==1.4.1`; printed venv python path |
| 2 | `./.venv-sc-publish/bin/python $SP/plugins/sc-publish/install.py --input release/sc-publish-consumer-input.json .` | 0 | 48 files copied, 2 manifests rendered |
| 3 | `./.venv-sc-publish/bin/python $SP/plugins/sc-publish/install.py --dry-run --input release/sc-publish-consumer-input.json .` | 0 | `Publish-kit assets are in sync.` — no drift |
| 4 | Byte-compare of every copied file vs package source (python sweep, same exclusion rules as `install.py`) | 0 | `byte-identical: 48, mismatched/missing: 0` |

Consumer-specific outputs are exactly the two rendered manifests plus the
caller-owned input:

| Rendered manifest | sha256 |
| --- | --- |
| `release/publish-artifacts.toml` | `f04d042c988be74e7386a5fe191b95e44a679456b9718810341dd602716a2407` |
| `release/publish-channel-contracts.toml` | `88d7ac055689c1e9fcd752c178eac9094ca29084a097fa7aff109c4fb0f40fd8` |

Expected overwrites of tracked legacy files (retained per AT.1 — deletion is
AT.3): `.github/workflows/release.yml`, `.github/workflows/release-preflight.yml`,
`.claude/agents/publisher.md`, `release/publish-artifacts.toml`. The legacy
`hermes-atm-pypi-publish.yml` is not touched by the kit and remains.

## Required Validation (as written in AT.1)

| # | Command (as written, placeholders substituted) | Exit | Result |
| --- | --- | --- | --- |
| V1 | `./.venv-sc-publish/bin/python plugins/sc-publish/install.py --dry-run --input release/sc-publish-consumer-input.json .` | 2 | **Fails as written**: `plugins/sc-publish/install.py` does not exist in the consumer (finding F1). Corrected form = install-evidence row 3 above (exit 0, no drift). |
| V2 | `diff -ru --exclude publish-artifacts.toml --exclude publish-channel-contracts.toml $SP/plugins/sc-publish/release ./release` | 1 | No content differences in kit-owned files; exit 1 solely from `Only in ./release:` entries for consumer/legacy files (`RELEASE-NOTES-TEMPLATE.md`, `publish-surface-scope.md`, `release-notes.md`, `sc-publish-consumer-input.json`) — finding F4. |
| V3 | `python3 .github/scripts/release_artifacts.py validate-manifest --manifest release/publish-artifacts.toml --workspace-toml Cargo.toml` | 0 | `manifest validation passed` |
| V4 | `python3 .just/check_version_sync.py` | 1 | **Fails**: `.github/workflows/release.yml no longer guarantees release wiring from the shared workspace version: missing 'update-homebrew:'` — finding F5. |

Package's own installer/unit-test suite (per AT.1, run instead of duplicating
it in ATM), executed against the installed copies in the worktree:

| Command | Exit | Result |
| --- | --- | --- |
| `./.venv-sc-publish/bin/python -m pytest -q .github/scripts/tests/` | 0 | `76 passed, 8 skipped, 3 subtests passed` (skips are consumer-context gates on `.sc-publish-source-root`); includes the mixed maturin+setuptools fixture and unsupported-build-system rejection tests in `test_install.py` |

## Post-install repo health (failures recorded, not fixed)

| Check | Exit | Result |
| --- | --- | --- |
| YAML parse of all 16 workflows + 5 kit actions | 0 | All parse |
| `python3 .just/lint_manifests.py` | 0 | `manifests passed` |
| `pytest .just/tests/test_release_artifacts.py test_validate_release.py test_release_preflight.py test_release_homebrew_workflow.py test_hermes_atm_pypi_publish_workflow.py test_prepare_hermes_atm_publish_artifacts.py test_release_gate.py` | 1 | **6 failed**, 19 passed (findings F5/F6) |
| Full `.just/tests` sweep (minus `test_benchmark_report.py`) | 1 | 10 failed, 369 passed: the same 6 release-workflow failures + 4 `test_atm_graft_python_models.py` failures that are environmental (`No module named 'pydantic'` in the rehearsal venv; also breaks `test_benchmark_report.py` collection) — not install fallout |

Failing tests caused by the kit adoption (all assert legacy workflow shape):

- `.just/tests/test_release_preflight.py` — 3 failures: expects the legacy
  `release-preflight.yml` steps (validation python deps install, staged
  install-root, staged installed-docs).
- `.just/tests/test_release_homebrew_workflow.py` — 3 failures: expects a
  legacy in-`release.yml` `update-homebrew:` job and
  `scripts/release_artifacts.py update-homebrew-formulas` invocation; the kit
  moves Homebrew to the post-release `homebrew-publish.yml` leg.

## Findings

| # | Severity | What the plan doc says | What reality required |
| --- | --- | --- | --- |
| F1 | medium | Required Validation V1: `<bootstrap-python> plugins/sc-publish/install.py --dry-run …`, implying the consumer holds the installer at `plugins/sc-publish/install.py`. | The installer copies itself to the **consumer root** (`install.py`), not under `plugins/sc-publish/`; the documented path exists only in the upstream repo checkout. Worse, the copied consumer-root `install.py` resolves `PACKAGE_ROOT` to its own directory, i.e. the whole consumer repo — running it would rglob the entire repo (including `.git`) and, pointed at another directory, would "install" the whole repo into it. Doc should state the dry-run runs from the pinned package checkout; upstream should consider excluding `install.py` from the copied set or installing it under a package-root marker (upstream `sc-publish` issue candidate, ADR-050). |
| F2 | low | Deliverable 1 / Required Document Updates: record the "package digest". | No digest algorithm or scope is defined anywhere in the phase docs. This receipt defines one (sha256 of the sorted per-file sha256 list over the 48 installable files); AT.1 proper should standardize it or cite a kit-provided digest command. |
| F3 | low | Deliverable 2: input covers "all release crates". | Ambiguous whether workspace-internal non-release crates (`atm-architecture`, `sc-lint-*`, test-support crates) must be declared with `publish=false`. Legacy manifest declared only the release surface plus `atm-graft-python`; this rehearsal follows that precedent and additionally declares `atm-query-python` (symmetric with graft, both are PyPI-released `publish=false` crates the legacy manifest's own comment says must not be silently omitted — the legacy manifest omitted query, arguably a legacy defect). AT.1 proper should state the rule explicitly. |
| F4 | low | Required Validation V2 diff command excludes only the two rendered manifests, and Acceptance Criteria expect it to demonstrate parity. | `release/` in a real consumer also holds consumer-owned/legacy files (`sc-publish-consumer-input.json` itself, `RELEASE-NOTES-TEMPLATE.md`, `publish-surface-scope.md`, `release-notes.md`), so the command always exits 1 on `Only in` entries even with byte-perfect parity. The doc should add `--exclude` entries for consumer-owned files or define the check as "no content diffs for kit-owned files" (satisfied here). |
| F5 | **high** | Required Validation V4: `python3 .just/check_version_sync.py` must pass; AT.1 declares the `release.yml` overwrite "is the adoption". | The repo-owned check hardcodes legacy `release.yml` fragments (`[version_sync.release_wiring].required_fragments` in `.just/lint-config.toml`: `update-homebrew:` etc.), so V4 **cannot pass** after the kit overwrites `release.yml`. AT.1 schedules no consumer-side update of `.just/lint-config.toml`; as written, the sprint's own Required Validation is unsatisfiable. AT.1 proper needs an explicit ATM-owned deliverable: retarget the version-sync release-wiring fragments (and F6 tests) to the kit workflows. |
| F6 | **high** | AT.1 claims parallel-safety and "remove no legacy workflow until AT.3", with no mention of repo test fallout. | 6 repo tests (`test_release_preflight.py` ×3, `test_release_homebrew_workflow.py` ×3) assert the legacy workflow shape and fail after the overwrite, which would fail CI on the AT.1 PR. These are consumer-owned tests (updating them is legitimate ATM work), but AT.1 does not schedule it. Same root cause as F5. |
| F7 | low | Phase README install contract runs `python plugins/sc-publish/.github/scripts/bootstrap_sc_compose.py` from the consumer repo root. | At AT.1 install time the consumer does not yet contain `plugins/sc-publish/` (chicken-and-egg): the bootstrap must run from the pinned upstream checkout the first time. After install, the bootstrap lands at `.github/scripts/bootstrap_sc_compose.py` (consumer root), not under `plugins/`. Doc should name the source explicitly. |
| F8 | info | Deliverable 3 names `release.yml` and `release-preflight.yml` as the same-named overwrites. | The installer also overwrites `.claude/agents/publisher.md` (pre-kit publisher agent, named in the phase README baseline) and `release/publish-artifacts.toml` (legacy manifest replaced by the rendered one). Both are correct adoption behavior; the AT.1 overwrite list is just incomplete. |

Notes, non-findings:

- The pinned schema accepted the spike draft unchanged; the two input edits
  came from the plan's explicitness requirement, not schema drift.
- `pytest`/`pyyaml` were added to the rehearsal venv for health checks only;
  the venv is not committed and bootstrap state (`sc-compose==1.4.1`) is
  unchanged.
- Rehearsal-only deviation: the receipt does not update the phase README pin
  (Required Document Updates) because plan docs live on
  `plan/phase-at-publish-recovery` and are read-only to this rehearsal; the pin
  there already names `0fa5b05e44a655ec76ada8a6c2b24714d47acca1`.
