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

**Always use `atm gh pr list` first** - single call, returns all open PRs with CI and merge state:

```bash
atm gh pr list
```

This is faster and sufficient for populating `sprint_rows` and `integration_row`. Only drill into individual `gh run view` calls if you need failure details for a specific job.

**Dogfooding rule**: If `atm gh pr list` output is missing information needed to fill the report (e.g., no per-job failure detail, no QA state, truncated CI summary), **file a GitHub issue** describing what field or format change would make it sufficient, then improve the command. Do not silently work around gaps with extra `gh` CLI calls - surface them as product issues.

## Render Command

The template path is relative - must run from the **main repo root** (not a worktree).

```bash
cd "${CLAUDE_PROJECT_DIR:-$(git worktree list | head -1 | awk '{print $1}')}"
echo '<json>' > /tmp/sprint-report.json
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
