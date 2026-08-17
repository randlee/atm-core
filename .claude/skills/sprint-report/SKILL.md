---
name: sprint-report
version: 1.1.0
description: Generate an authoritative sprint status report from an integrate/phase-* worktree and repair reported TTL, QA, or GitHub source-data gaps before rendering. Use for sprint status, phase reports, report data gaps, or `/sprint-report`.
---

# Sprint Report

Generate canonical JSON, repair every reported gap at its source, then render.
`mode` controls table versus detailed output.

## Usage

```
/sprint-report [--table | --detailed]
```

Default: `--table`

---

## Step 1 — Verify CLI dependencies

```bash
which sc-compose && sc-compose --version
python3 -c 'import rdflib'
```

If either check fails, read
[`references/installation-and-troubleshooting.md`](references/installation-and-troubleshooting.md),
repair the environment, and rerun these checks before continuing.

## Step 2 — Produce the canonical source

Use the canonical triage report. It resolves the current `integrate/phase-*`
worktree and reads only that phase's Turtle findings; never assemble counts from
a sprint worktree, a QA snapshot, or a hand-maintained metadata file.

```bash
python3 .claude/skills/triage-report/scripts/triage_report.py \
  --phase AICH --format vars > /tmp/sprint-report.json
```

The command includes current GitHub PR/CI state.

## Step 3 — Repair a nonzero result

For either `kind: "data_gap"` (exit 3) or `kind: "error"` (exit 2), do not
render, substitute values, or switch source worktrees. Read the single repair
authority: [TTL Repair Guide](../../../docs/triage/ttl-repair.md).

For `data_gap`, process every JSON `remediations[]` item:

1. Use its `target_branch` integration worktree and repair its exact `path`.
2. Perform the direct `action`, then run the validation required by the guide.
3. Commit and land the source correction through the normal PR/QA path.
4. Rerun the command. Repeat until it exits zero.

For `error`, follow `error.suggested_action` and the same guide. A structural
failure is repaired in source, not explained away in the report.

## Step 4 — Render only a zero-exit report

The template path is relative - must run from the **main repo root** (not a worktree).

```bash
cd "${CLAUDE_PROJECT_DIR:-$(git worktree list | head -1 | awk '{print $1}')}"
sc-compose render .claude/skills/sprint-report/report.md.j2 --var-file /tmp/sprint-report.json
```

## --table (default)

```json
{
  "mode": "table",
  "sprint_rows": "| AK.1 | ✅ | ✅ | 🏁 | #621 |\n| AK.2 | ✅ | ✅ | 🌀 | #622 |",
  "integration_row": "| **integrate** | | — | 🌀 | — |"
}
```

## --detailed

```json
{
  "mode": "detailed",
  "sprint_rows": "Sprint: AK.1  Contract reconciliation\nPR: #621\nQA: PASS ✓ (iter 3)\nCI: Merged to integrate/phase-AK ✓\n────────────────────────────────────────\nSprint: AK.2  OTel core\nPR: #622\nQA: PASS ✓\nCI: Running (1 pending)",
  "integration_row": "Integration: integrate/phase-AK → develop\nCI: Running — pending AK.4 + AK.5"
}
```

## Icon Reference

| State | DEV | QA | CI |
|-------|-----|----|----|
| Assigned | 📥 | 📥 | |
| In progress | 🌀 | 🌀 | 🌀 |
| Done/Pass | ✅ | ✅ | ✅ |
| Findings | 🚩 | 🚩 | |
| Fixing | 🔨 | | |
| Blocked | | | 🚧 |
| Fail | | | ❌ |
| Merged | | | 🏁 |
| Ready to merge | | | 🚀 |
