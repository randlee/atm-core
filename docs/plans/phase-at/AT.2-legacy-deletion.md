# AT.2 — Legacy Publish Surface Deletion

```yaml
plan_type: sprint_plan
phase: AT
sprint: AT.2
worktree: feature/pat-s2-legacy-publish-deletion
branch: feature/pat-s2-legacy-publish-deletion
status: complete
estimated_scope: verify-then-delete of retired publish assets; no behavior additions
```

## Goal

Remove every deprecated publish asset that the installed shared kit now
covers, so the kit files plus the ATM-owned consumer input are the entire
publish surface. Deletion is gated on AT.1's successful run as coverage
evidence (ADR-050: ATM owns no local copies of generic release behavior).

## Deliverables

1. A resolved disposition for every Deletion Candidates row, with the
   reference-scan output and coverage statement recorded in the receipt.
2. The deletions themselves, plus removal of now-dead references (Justfile
   recipes, test registrations, doc links).
3. Retained-file rationales recorded in module docstrings and in the PR
   description.
4. The AT.2 receipt at `docs/plans/phase-at/receipts/AT.2-receipt.md`
   (shape per the phase README's Receipt Convention).

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

Files the installer overwrites in place — the complete rehearsal-proven list:
`.github/workflows/release.yml`, `.github/workflows/release-preflight.yml`,
`.claude/agents/publisher.md`, and `release/publish-artifacts.toml` — are
adopted in AT.1 and are not deletion rows (the kit's channel-agent prompts
are installed as new files, not overwrites). AT.1's receipt lists every file
the installer created or overwrote (49 files at the pin), including the
installed agent files. This sprint owns what the installer does **not**
replace by name.

The `Gate` column names the specific receipt that must exist before that
row may be deleted:

- **Production PyPI receipt** = the first kit-driven release's receipt with
  its production PyPI section complete (not
  `production: pending-authorization`).
- **First kit-release receipt** = the receipt of the first kit-driven
  release proving the crates.io + GitHub Release legs end-to-end.
- **Homebrew / winget channel receipt** = that same first kit-driven
  release's homebrew / winget channel receipt sections.

| Path | Observed state | Expected coverage | Gate |
| --- | --- | --- | --- |
| `.github/workflows/hermes-atm-pypi-publish.yml` | Legacy bespoke PyPI workflow for `hermes-atm` | Kit `pypi-publish.yml` building the declared setuptools distribution (upstream #39) | Production PyPI receipt |
| `scripts/prepare_hermes_atm_publish_artifacts.py` | Feeds the legacy hermes workflow | Same kit leg; manifest-declared distribution | Production PyPI receipt (deleted with the workflow it feeds) |
| `.just/tests/test_hermes_atm_pypi_publish_workflow.py` | Tests the legacy hermes workflow | Kit upstream tests at the pinned revision | Production PyPI receipt (same gate as its subject) |
| `.just/tests/test_prepare_hermes_atm_publish_artifacts.py` | Tests the legacy prepare script | Same | Production PyPI receipt (same gate as its subject) |
| `scripts/release_artifacts.py` | Legacy copy at root; kit installs `.github/scripts/release_artifacts.py` | Kit script | First kit-release receipt |
| `scripts/release_gate.sh` | Legacy copy at root | Kit `.github/scripts/release_gate.sh` via `release.yml` | First kit-release receipt |
| `scripts/verify_release_archive.py` | Legacy archive verifier | Kit `verify-published-release` action + `verify-*-release-assets` subcommands; retain only if it checks ATM-owned data the kit does not | First kit-release receipt |
| `scripts/validate_release.py` | **Live** — invoked by the Justfile validate recipe | Decision required: replace the recipe with kit `validate-manifest`/preflight equivalents, or declare the script ATM-owned consumer validation and record that explicitly | First kit-release receipt (in addition to the recorded decision) |
| `.just/tests/test_release_artifacts.py` | Exercises legacy release-asset behavior | Kit upstream test suite | First kit-release receipt |
| `.just/tests/test_release_gate.py` | Exercises legacy `release_gate.sh` | Kit upstream test suite | First kit-release receipt |
| `.just/tests/test_release_homebrew_workflow.py` | Asserts homebrew channel-config against the workflow (expectations retargeted in AT.1 Deliverable 7) | Kit upstream tests; keep only if asserting ATM-owned manifest *data* | Homebrew channel receipt |
| `.just/tests/test_release_preflight.py` | Asserts against `release-preflight.yml` + `validate_release.py` (expectations retargeted in AT.1 Deliverable 7) | Kit upstream tests; same data-vs-behavior split | First kit-release receipt |
| `.just/tests/test_validate_release.py` | Tests `scripts/validate_release.py` | Follows the `validate_release.py` decision | First kit-release receipt (follows its subject's gate) |
| `.winget/randlee.agent-team-mail.yaml` | Local winget manifest | Kit `winget-publish.yml` generates manifests; delete if fully replaced, else record an ATM-owned rationale | Winget channel receipt |

Any additional legacy file discovered during the sprint joins the table with
the same loop and an explicit gate; none is deleted without its row.

Rows whose gate receipt does not yet exist when AT.2 executes are RETAINED
with status `deferred-until-<gate>` recorded in the AT.2 receipt (e.g.
`deferred-until-first-kit-release-receipt`). AT.2 completes with these
explicit deferrals rather than waiting on future releases; a follow-up
deletion pass runs after the first full kit-driven release produces the
outstanding gate receipts.

Ambiguous-coverage rows ("decision required") are resolved by a written
rationale in the sprint PR description; the quality-mgr gate blocks merge
until it confirms the rationale. No separate architecture sign-off is
required.

## Test Ownership Rule

The split for `.just` release tests: a test that re-verifies **shared kit
behavior** is deleted (upstream owns that suite at the pinned revision); a
test that verifies **ATM-owned data** (manifest declarations, version sync,
doc review gates) is retained and re-pointed at the rendered manifests. Each
retained test records that rationale in its module docstring.

## Prerequisites

**Amendment (2026-08-27, owner decision — forward-only publishing).** Rand
ruled that re-publishing tags predating the kit installation is out of scope
("I don't see a reason to try to re-publish anything before the first kit
install"). `v1.4.3` is unpublishable via the kit (the kit action and manifest
schema are absent/incompatible at that tag; see the AT.1 receipt's TestPyPI
amendment). All publication evidence for this sprint's gates therefore comes
from the **first kit-era release (workspace version `1.4.4`)**, cut from a
kit-installed tree after phase AT merges to `develop`. This sprint executes
now, within phase AT: rows whose gate receipt does not yet exist are RETAINED
as `deferred-until-<gate>` per the Deletion Candidates deferral rule above,
and the follow-up deletion pass after the first kit-era release resolves
them. The sprint does not wait for that release.

- Gate evidence for **deleting** a publish-path row is the first kit-era
  release's TestPyPI receipt proving the kit built and published every
  declared distribution — including the setuptools `hermes-atm` —
  end-to-end. The hermes rows (the
  `.github/workflows/hermes-atm-pypi-publish.yml` workflow, its feeder
  script, and their tests) additionally require the production PyPI receipt
  (the receipt's production section complete, not
  `production: pending-authorization`), per the `Gate` column. Rows whose
  gate receipt does not yet exist when the sprint runs are deferred, not
  deleted.
- A channel-scoped credential-unavailable (fail-closed) result in AT.1 is
  valid evidence the workflow fails closed, and per the phase README
  Non-Blocking Outcomes table it is never an emergency — but it does NOT
  constitute publication and does not satisfy a row's gate. A row is deleted
  only on receipts showing the public indexes actually contain the first
  kit-era (1.4.4) distributions; otherwise it is retained as
  `deferred-until-<gate>`.
- `install.py --dry-run` (run from the pinned checkout, per the phase README
  installer contract) clean on the sprint branch before starting.

## Dependencies

- `AT.1`: `must_follow`. Deletion without the coverage proof recreates
  the risk ADR-050 exists to prevent, in the opposite direction.

## Non-Goals

- No new validation logic, no workflow edits, no consumer-input changes beyond
  removing references to deleted files.
- No deletion inside kit-installed paths — those are upstream-owned and only
  change through the installer.

## Acceptance Criteria

- Every table row resolved: deleted with its gate receipt, reference scan,
  and coverage statement recorded; explicitly retained with an ATM-ownership
  rationale; or retained as `deferred-until-<gate>` where the row's gate
  receipt does not yet exist. No row is deleted before its gate receipt.
- `install.py --dry-run` still reports no drift after all deletions.
- Full repo lint and test gates pass; no dangling references to deleted paths
  (`grep` scans return empty).
- The AT.2 receipt (`docs/plans/phase-at/receipts/AT.2-receipt.md`) lists
  every deleted path, every retained-with-rationale path, and every
  `deferred-until-<gate>` path, with a coverage statement (or named pending
  gate) for each.

## Required Validation

```bash
# Reference scan, run per candidate path (must return empty after deletion)
grep -rn "<path>" .github/ .just/ Justfile scripts/ docs/ --include="*.yml" --include="*.py" --include="Justfile" --include="*.md"
# Kit dry-run via the pinned package checkout (must stay clean; never the
# vendored consumer-root install.py — sc-publish#46)
<venv>/bin/python <sc-publish-checkout>/plugins/sc-publish/install.py --dry-run --input release/sc-publish-consumer-input.json <worktree>
# Repo gates (Justfile: `lint` runs .just/run_lint.py, `test` runs .just/run_tests.py)
just lint
just test
# YAML-parse every remaining workflow
python3 -c "import pathlib, yaml; [yaml.safe_load(p.read_text()) for p in pathlib.Path('.github/workflows').glob('*.yml')]"
```

## Risks And Watchouts

- A `.just` test may guard ATM-owned data despite living next to legacy
  behavior tests — misclassifying it deletes real coverage. The data-vs-
  behavior split is the review gate, applied per test class, not per file.
- `scripts/validate_release.py` drives a live Justfile recipe; deleting it
  without replacing the recipe breaks `just` targets. Resolve the decision
  first, then delete.
- The hermes-atm legacy workflow must not be deleted before the production
  PyPI receipt proves the kit publishes the setuptools distribution — the
  TestPyPI receipt alone does not clear that row, and the proof depends on
  the upstream #39 fix being in the pinned revision.
- Classify every non-passing result against the phase README's Non-Blocking
  Outcomes table before reacting; only its GENUINE STOP row halts work.
  Legacy `.just` test failures after install are that table's expected,
  split-by-kind row: expectation updates for overwritten workflows were
  AT.1's work item (Deliverable 7); retention-vs-deletion of those test
  files is resolved here through this sprint's data-vs-behavior split.
