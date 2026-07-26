---
name: triage-report
version: 1.0.0
description: Produce an auditable phase triage report from the integration worktree source of truth.
---

# Triage Report

`triage_report.py` is the single producer for this report. Its JSON output is
the machine-readable source of truth and its precomputed row strings are the
inputs displayed by `sc-compose`; Jinja performs no gate arithmetic.

## Usage

Run from the current `integrate/phase-*` worktree, or pass the worktree
explicitly:

```bash
python3 -m pip install --upgrade rdflib
python3 .claude/skills/triage-report/scripts/triage_report.py \
  --phase AICH --format table
python3 .claude/skills/triage-report/scripts/triage_report.py \
  --phase AICH --format json > /tmp/triage-report.json
```

When the command is not run from an integration worktree it searches Git
worktrees. It fails with a structured JSON error (exit 2) when there is not
exactly one `integrate/phase-*` candidate. Use `--integration-root` to remove
that ambiguity. `--qa-master` and `--metadata` may be supplied when those
artifacts are stored at non-default paths.

The default QA evidence path is
`docs/plans/phase-<phase>/.audit/qa-evidence-master.json`. Only the latest
`run_type: "qa"` run per sprint is authoritative; reviewer-only runs do not
replace QA. Missing QA, PR, CI, branch, acknowledgement, or merge inputs stay
`null` and are listed in `data_gaps`.

The QA message/shared/temp path fields in machine rows are retained as
host-local evidence pointers from the audit master for drill-down. They are
not canonical Turtle paths; triage record paths remain repository-relative.

## Calculated gates

- `ready_to_merge` is true only when the authoritative blocker count is known
  and zero; unknown counts produce `null`.
- `ok_to_merge` is true only when `ready_to_merge` is true and every earlier
  sprint has explicit `merged: true` metadata. No branch name, PR number, or
  ancestry is treated as proof of merge.
- `quality_gate` separately requires all known B/I/M counts to be zero and is
  never used to silently convert missing counts to zero.

The table uses the same DEV/QA/CI/PR icons as `/sprint-report`: `📥`, `🌀`,
`✅`, `🚩`, `🔨`, `🚧`, `❌`, `🏁`, and `🚀`. Unknown values are shown as `?` or
`—`, not guessed by the agent.

## Rendering through sc-compose

The JSON contains `mode`, `phase`, `sprint_rows`, `integration_row`, and
`detailed_rows` variables for the templates in this directory:

```bash
# Install the standalone CLI from the platform release channel first, e.g.:
brew install randlee/tap/sc-compose
sc-compose --version  # must be 1.2.0 or newer; no upper bound is enforced
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
