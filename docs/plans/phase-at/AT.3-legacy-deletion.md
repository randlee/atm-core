# AT.3 — Legacy Publish Surface Deletion

```yaml
plan_type: sprint_plan
phase: AT
sprint: AT.3
worktree: feature/pat-s3-legacy-publish-deletion
branch: feature/pat-s3-legacy-publish-deletion
status: proposed
estimated_scope: verify-then-delete of retired publish assets; no behavior additions
```

## Goal

Remove every deprecated publish asset that the installed shared kit now
covers, so the kit files plus the ATM-owned consumer input are the entire
publish surface. Deletion is gated on AT.2's successful run as coverage
evidence (ADR-050: ATM owns no local copies of generic release behavior).

## Method

Every candidate follows the same verify-then-delete loop; no candidate is
deleted on assumption:

1. **Reference scan** — prove nothing live invokes the path:

   ```bash
   grep -rn "<path>" .github/ .just/ Justfile scripts/ docs/ --include="*.yml" --include="*.py" --include="Justfile" --include="*.md"
   ```

2. **Coverage statement** — name the installed kit feature (file plus
   subcommand/job) that provides the retained behavior, citing AT.1/AT.2
   evidence.
3. **Delete** the file and any now-dead references (Justfile recipes, test
   registrations, doc links).
4. **Re-run** the repo lint/test gates and the kit dry-run (must stay clean).

## Deletion Candidates (baseline `develop` @ `d610b4c07`, post-PR #960)

Files the installer overwrites in place (`release.yml`,
`release-preflight.yml`, `.claude/agents/publisher.md` and channel agents,
`release/publish-artifacts.toml`) are adopted in AT.1 and are not deletion
rows. This sprint owns what the installer does **not** replace by name:

| Path | Observed state | Expected coverage |
| --- | --- | --- |
| `.github/workflows/hermes-atm-pypi-publish.yml` | Legacy bespoke PyPI workflow for `hermes-atm` | Kit `pypi-publish.yml` building the declared setuptools distribution (upstream #39) |
| `scripts/prepare_hermes_atm_publish_artifacts.py` | Feeds the legacy hermes workflow | Same kit leg; manifest-declared distribution |
| `.just/tests/test_hermes_atm_pypi_publish_workflow.py` | Tests the legacy hermes workflow | Kit upstream tests at the pinned revision |
| `.just/tests/test_prepare_hermes_atm_publish_artifacts.py` | Tests the legacy prepare script | Same |
| `scripts/release_artifacts.py` | Legacy copy at root; kit installs `.github/scripts/release_artifacts.py` | Kit script |
| `scripts/release_gate.sh` | Legacy copy at root | Kit `.github/scripts/release_gate.sh` via `release.yml` |
| `scripts/verify_release_archive.py` | Legacy archive verifier | Kit `verify-published-release` action + `verify-*-release-assets` subcommands; retain only if it checks ATM-owned data the kit does not |
| `scripts/validate_release.py` | **Live** — invoked by the Justfile validate recipe | Decision required: replace the recipe with kit `validate-manifest`/preflight equivalents, or declare the script ATM-owned consumer validation and record that explicitly |
| `.just/tests/test_release_artifacts.py` | Exercises legacy release-asset behavior | Kit upstream test suite |
| `.just/tests/test_release_gate.py` | Exercises legacy `release_gate.sh` | Kit upstream test suite |
| `.just/tests/test_release_homebrew_workflow.py` | Asserts homebrew channel-config against the workflow | Kit upstream tests; keep only if asserting ATM-owned manifest *data* |
| `.just/tests/test_release_preflight.py` | Asserts against `release-preflight.yml` + `validate_release.py` | Kit upstream tests; same data-vs-behavior split |
| `.just/tests/test_validate_release.py` | Tests `scripts/validate_release.py` | Follows the `validate_release.py` decision |
| `.winget/randlee.agent-team-mail.yaml` | Local winget manifest | Kit `winget-publish.yml` generates manifests; delete if fully replaced, else record an ATM-owned rationale |
| `docs/plans/publish-kit-migration/` | AS-era migration plan directory | Phase AT plan documents |

Any additional legacy file discovered during the sprint joins the table with
the same loop; none is deleted without its row.

## Test Ownership Rule

The split for `.just` release tests: a test that re-verifies **shared kit
behavior** is deleted (upstream owns that suite at the pinned revision); a
test that verifies **ATM-owned data** (manifest declarations, version sync,
doc review gates) is retained and re-pointed at the rendered manifests. Each
retained test records that rationale in its module docstring.

## Prerequisites

- AT.2 receipts exist (the shared path demonstrably published a real channel
  end-to-end, including the setuptools `hermes-atm` distribution).
- `install.py --dry-run` clean on the sprint branch before starting.

## Dependencies

- `AT.1`, `AT.2`: `must_follow`. Deletion without the coverage proof recreates
  the risk ADR-050 exists to prevent, in the opposite direction.

## Non-Goals

- No new validation logic, no workflow edits, no consumer-input changes beyond
  removing references to deleted files.
- No deletion inside kit-installed paths — those are upstream-owned and only
  change through the installer.

## Acceptance Criteria

- Every table row resolved: deleted with its reference scan and coverage
  statement recorded, or explicitly retained with an ATM-ownership rationale.
- `install.py --dry-run` still reports no drift after all deletions.
- Full repo lint and test gates pass; no dangling references to deleted paths
  (`grep` scans return empty).
- The phase receipt lists every deleted path and every retained-with-rationale
  path.

## Risks And Watchouts

- A `.just` test may guard ATM-owned data despite living next to legacy
  behavior tests — misclassifying it deletes real coverage. The data-vs-
  behavior split is the review gate, applied per test class, not per file.
- `scripts/validate_release.py` drives a live Justfile recipe; deleting it
  without replacing the recipe breaks `just` targets. Resolve the decision
  first, then delete.
- The hermes-atm legacy workflow must not be deleted before AT.2 proves the
  kit publishes the setuptools distribution — that proof depends on the
  upstream #39 fix being in the pinned revision.
