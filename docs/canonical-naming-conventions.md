# Canonical phase and sprint naming

ATM Core consumes the shared naming contract maintained by Synaptic Canvas:

**[Synaptic Canvas ATM naming conventions (SSOT)](https://github.com/randlee/synaptic-canvas/blob/develop/docs/ATM-NAMING-CONVENTIONS.md)**

This file is an integration pointer, not a second policy document.  Do not
copy or fork the phase, sprint, plan, branch, worktree, or TTL grammar here.
Changes to those rules belong in the Synaptic Canvas SSOT and should be
reviewed there first.

## ATM Core integration boundary

ATM Core applies the SSOT at ingestion and validation boundaries:

- incoming phase/sprint identifiers are trimmed and compared
  case-insensitively;
- accepted legacy forms are converted to the SSOT's canonical value before
  persistence;
- persisted plan, report, and TTL data uses the canonical case and separator;
- the original input is retained in diagnostics when normalization occurs; and
- ambiguous or unknown forms fail with an actionable diagnostic instead of
  being reported as a missing run.

The executable TTL validator is
`.just/lint_ttl_triage_consistency.py`.  Its sprint-key diagnostics include
`TTL.QA_RUN_KEY_MISMATCH`, `NAMING.NON_CANONICAL`,
`NAMING.LEGACY_IDENTIFIER`, and `NAMING.UNKNOWN_SPRINT_FORMAT`.

## Repository examples

These examples show how ATM Core refers to the SSOT without redefining it:

| Artifact | Canonical ATM Core example |
| --- | --- |
| phase directory | `docs/plans/phase-an/` |
| sprint plan | `docs/plans/phase-an/sprint-AN.8-validation-evidence.md` |
| sprint metadata | `sprint: AN.8` and the actual `branch`/`worktree` values |
| TTL/report linkage | `triage:foundIn triage:AN.8` and `aich_sprint: "AN.8"` |

Historical files may use older spellings.  They are tracked in the migration
inventory and must not be used as templates for new plans or evidence.

