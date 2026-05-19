# Step 2

Route to: `plan-scope-reviewer`

Purpose:
- review sprint split, ownership, production-ready scope, and direct
  dev/QA consumability

Action:
- use Agent tool to launch `.claude/agents/plan-scope-reviewer.md`
- pass `step-1` fenced JSON as required input
- set `run_in_background: true`
- wait for `step-2` fenced JSON

Required input:
- `step-1` fenced JSON
- current planning docs
- `.claude/skills/plan-hardening/sprint-planning-guidelines.md`

Expected output:
- `step-2` fenced JSON findings

Hard stops:
- missing or malformed `step-1` JSON
- reviewer output missing or malformed
