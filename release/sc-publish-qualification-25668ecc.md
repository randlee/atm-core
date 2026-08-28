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
