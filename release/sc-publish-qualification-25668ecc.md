# sc-publish qualification receipt — revision 25668ecc164261be676c9414c4f603b18ab74c91

- **Candidate revision**: `25668ecc164261be676c9414c4f603b18ab74c91` (sc-publish `main`, merge of PR #64
  `develop → main`; develop is an ancestor of main; content = reconciliation PR #63:
  main 43552e4 + develop 6ee8d88 + release-candidate git-identity fix, plus B1–B4
  pin/bootstrap/qualification conventions at 928c8f9)
- **Consumer repository**: atm-core at `origin/develop` = `3b5de579d`
- **Qualified by**: fenix (atm-dev), 2026-08-27
- **Procedure**: AT.1-style consumer qualification per `README.sc-publish.md`
  "Pinning, bootstrap, and qualification" (wyvern B4 checklist)

## Evidence

| Leg | Result |
|-----|--------|
| Isolated bootstrap (B2) | kit cloned to repo-local `.sc-publish-kit/` at the candidate SHA; `bootstrap_sc_compose.py --venv .venv-kit` provisioned pinned **sc-compose 1.5.0** |
| Clean install | `install.py --input release/sc-publish-consumer-input.json` completed clean (last copy: `release/sc-publish-pin.toml.example`) |
| Repeat dry-run | exit 0 — "Publish-kit assets are in sync." (no drift) |
| Byte-parity sweep | byte-identical: **54**, mismatched/missing: **0** (kit grew 49 → 54 files vs pin 42e0fce) |
| Manifest validation | `release_artifacts.py validate-manifest` — "manifest validation passed" |
| Kit test suite (pinned renderer) | `pytest .github/scripts/tests/` — **101 passed, 8 skipped, 3 subtests passed, 1 failed** (exception recorded below) |
| Live release-candidate tag cut | pending — satisfied by the atm-core v1.4.4 candidate cut with the fixed `release-candidate.yml` |
| Post-release leg retry | pending — satisfied by a v1.4.4 post-release channel leg retry |

## Deferred-leg authorization (ATM-QA-017)

The README.sc-publish.md qualification gate requires a live release-candidate
tag cut and one post-release channel leg retry before any pin advance. For this
specific advance those two legs are circular by construction: the revision being
qualified contains the `release-candidate.yml` git-identity fix, and no candidate
tag can be cut on the broken previous pin (42e0fce fails closed, run
33135181716). The gate text has no carve-out for a pin advance that itself
unblocks the release; because README.sc-publish.md is a byte-copied kit file
(ADR-050), the carve-out cannot be added in this consumer without creating
drift — it is filed upstream for the next qualified revision (sc-publish #66).

Disposition: the tag-cut and post-release-retry legs are **explicitly deferred**
to the atm-core v1.4.4 release this pin unblocks; both results will be appended
to this receipt when they complete. Authorization for the deferred-leg pin
advance is Rand's approval of atm-core PR #1069 (develop merges require user
approval; the approval act is recorded on the PR itself). If either deferred leg
fails, the pin does not bless forward to other consumers until it is re-run
clean.

## Recorded exception

`test_publish_kit_scripts.py::ReleaseScriptTests::test_runtime_renderer_paths_use_the_bootstrapped_exact_pin`
fails in any consumer repository by construction: it resolves the repo root as
`PACKAGE_ROOT.parents[1]` (assumes the kit repo's `plugins/sc-publish/` nesting) and
inspects the sc-publish repository's own `.github/workflows/ci.yml` and root
`README.md`, which are not kit assets. In a consumer, `install.py` is at the repo
root, so `PACKAGE_ROOT.parents[1]` escapes the checkout →
`FileNotFoundError`. The test is new since pin 42e0fce and passes in the kit repo
itself (full suite: 100 passed, 10 skipped on the reconcile branch under the pinned
renderer). Not a kit-behavior defect; atm-core CI does not execute the kit test
suite, so no consumer CI impact. Filed upstream as
[sc-publish #65](https://github.com/randlee/sc-publish/issues/65) for the next
qualified revision; no kit change made mid-qualification per the multi-repo
qualification rule.

## Release-branch fixes (release/v1.4.4 only — documented kit drift)

The v1.4.4 final Release Preflight (run 33150049684, main tip 78913af39)
surfaced three defects in the kit's own workflows — the first real execution
of these paths on a crates-publishing consumer. Per Rand's rulings
(2026-08-28): sc-lint is a lint tool owned by repo CI (`just lint` runs it —
the vendored workspace analyzer — extensively, every sprint); sc-publish is
git workflows and agent prompts; the publish pipeline must run the same
lint/build/test sprint CI runs plus only publish-specific checks, and there
must be NO surprises at publish time. Fixes applied directly on
`release/v1.4.4` (commit 9f5bae568), bypassing `develop` per the
release-state strategy's release-fix path:

1. Removed the sc-lint install/smoke gate from
   `.github/actions/setup-lint-toolchain` — preflight was downloading
   published sc-lint 0.4.0 while the repo runs the vendored workspace
   analyzer (version/schema skew). Upstream deletion:
   [sc-publish #67](https://github.com/randlee/sc-publish/issues/67).
2. Implemented the `crates_io` credential-liveness arm in
   `release-preflight.yml` (read-only probe of `https://crates.io/api/v1/me`).
   Upstream: [sc-publish #68](https://github.com/randlee/sc-publish/issues/68).
3. Added `if: always()` to the five evidence steps that skipped on an earlier
   hard failure, so one run collects complete evidence. Upstream:
   [sc-publish #69](https://github.com/randlee/sc-publish/issues/69).

These are deliberate, temporary drift from pin 25668ecc, scoped to the
release branch; `develop` stays byte-clean against the pin and re-converges
on the next qualified sc-publish revision. All preflight gates were rehearsed
locally green before any CI dispatch (fmt, clippy, workspace tests,
validate-manifest, publish-order, verify-version, version-lockstep, helper
presence; the crates_io liveness probe and registry-state checks execute
first in CI since publish tokens exist only as Actions secrets).
