---
name: triage-report
version: 1.1.0
description: Produce an auditable phase triage report from an integrate/phase-* worktree, including sprint findings, QA provenance, PR/CI state, and actionable source-data repairs. Use for triage reports, phase findings, report data gaps, or `/triage-report`.
---

# Triage Report

`triage_report.py` is the single producer for this report. Its JSON output is
the machine-readable source of truth and its precomputed row strings are the
inputs displayed by `sc-compose`; Jinja performs no gate arithmetic.

Each sprint row's live B/I/M counts and merge readiness come from unresolved
Turtle findings through graph-orchestration's existing
`open-findings-for-sprint.sparql` query. QA evidence is shown as review
provenance only. Current phase-wide findings are deduplicated in a separate
integration summary, so later occurrences do not distort the sprint's QA
snapshot. Fixed findings remain fixed; an open occurrence beneath one is TTL
reconciliation data and never becomes a development blocker. PR, CI, and merge
state are read from GitHub for the branch explicitly declared in Turtle; no
branch is inferred from a criteria filename and no hand-maintained metadata
file is used.

## Step 1 — Verify report dependencies

```bash
which python3 && python3 -c 'import rdflib'
which gh && gh auth status
```

If either check fails, read
[`references/installation-and-troubleshooting.md`](references/installation-and-troubleshooting.md),
repair the environment, and rerun these checks. Do not generate a partial
report with unknown GitHub state.

## Usage

Run from the current `integrate/phase-*` worktree, or pass the worktree
explicitly:

```bash
python3 .claude/skills/triage-report/scripts/triage_report.py \
  --phase AICH --format table
python3 .claude/skills/triage-report/scripts/triage_report.py \
  --phase AICH --format json > /tmp/triage-report.json
```

When the command is not run from an integration worktree it searches Git
worktrees. It fails with a structured JSON error (exit 2) when there is not
exactly one `integrate/phase-*` candidate. Use `--integration-root` to remove
that ambiguity. `--qa-master` may be supplied when QA evidence is stored at a
non-default path.

The default QA evidence path is
`docs/plans/phase-<phase>/.audit/qa-evidence-master.json`. Only the latest
`run_type: "qa"` run per sprint is displayed as QA provenance; reviewer-only
runs do not replace QA.

## Repair a nonzero result

The report exits `3` with `kind: "data_gap"` when source data is incomplete,
or `2` with `kind: "error"` for a structural source failure. Neither result
is renderable or dispatchable.

Read the single repair authority: [TTL Repair Guide](../../../docs/triage/ttl-repair.md).
For a data gap, execute every `remediations[]` item in its named
`target_branch` worktree: repair its exact `path`, perform its `action`,
validate, land the correction through normal PR/QA, and rerun until exit zero.
For an error, follow `error.suggested_action` and the same guide. Never repair
a sprint worktree, fabricate fields in report output, or infer a branch.

The QA message/shared/temp path fields in machine rows are retained as
host-local evidence pointers from the audit master for drill-down. They are
not canonical Turtle paths; triage record paths remain repository-relative.

## Calculated gates

- `ready_to_merge` is true only when the live unresolved Turtle blocker count
  is zero; it is not applicable once GitHub reports the sprint merged.
- `ok_to_merge` is true only when `ready_to_merge` is true and GitHub reports
  every earlier sprint's current delivery attempt as merged; it is not
  applicable once the sprint itself is merged.
- `quality_gate` requires live unresolved Turtle B/I/M counts to be zero.

## Findings and evidence displayed

- Per-sprint table and detailed rows show live unresolved B/I/M, QA verdict
  provenance, PR/CI/merge state, and merge gates.
- `diagnostics` identifies malformed or invalid finding records with their
  source paths and repair action.
- The integration summary shows deduplicated current B/I/M separately from
  historical sprint QA. A stale open occurrence under a fixed finding is a
  reconciliation diagnostic, never a fabricated development blocker.

The table uses the same DEV/QA/CI/PR icons as `/sprint-report`: `📥`, `🌀`,
`✅`, `🚩`, `🔨`, `🚧`, `❌`, `🏁`, and `🚀`. Unknown values are shown as `?` or
`—`, not guessed by the agent.

## Rendering through sc-compose

The JSON contains `mode`, `phase`, `sprint_rows`, `integration_row`, and
`detailed_rows` variables for the templates in this directory:

```bash
which sc-compose && sc-compose --version
sc-compose render --root . --file .claude/skills/triage-report/report.md.j2 \
  --var-file <(python3 .claude/skills/triage-report/scripts/triage_report.py \
    --phase AICH --format vars)
```

Use the same `--mode detailed --format vars` projection with
`report-detailed.md.j2` for the detailed template view.

```bash
sc-compose render --root . --file .claude/skills/triage-report/report-detailed.md.j2 \
  --var-file <(python3 .claude/skills/triage-report/scripts/triage_report.py \
    --phase AICH --mode detailed --format vars)
```

`--format vars` is a scalar-only projection of the same canonical result for
the `sc-compose` var-file boundary. The report script remains the calculation
boundary even when a newer `sc-compose` release is installed; the template
only renders its values.
