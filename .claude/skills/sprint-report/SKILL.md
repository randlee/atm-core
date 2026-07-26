---
name: sprint-report
description: Generate a sprint status report for the current phase. Default is --table.
---

# Sprint Report Skill

Build fenced JSON and pipe to the Jinja2 template. `mode` controls table vs detailed.

## Usage

```
/sprint-report [--table | --detailed]
```

Default: `--table`

---

## Data Source

Use the canonical triage report. It resolves the current `integrate/phase-*`
worktree and reads only that phase's Turtle findings; never assemble counts from
a sprint worktree, a QA snapshot, or a hand-maintained metadata file.

```bash
python3 .claude/skills/triage-report/scripts/triage_report.py \
  --phase AICH --format vars > /tmp/sprint-report.json
```

The command includes current GitHub PR/CI state. If its JSON reports a data
gap, show it as unknown rather than substituting another data source.

## Render Command

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
