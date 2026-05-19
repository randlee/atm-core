# Step 4

Route to: `critical-plan-reviewer`

Purpose:
- attack architecture risk, boundary risk, and false closure after
  sprint-scope hardening

Action:
- use Agent tool to launch `.claude/agents/critical-plan-reviewer.md`
- pass `step-3` fenced JSON as required input
- set `run_in_background: true`
- wait for `step-4` fenced JSON

Required input:
- `step-3` fenced JSON
- current planning docs
- `.claude/skills/plan-hardening/sprint-planning-guidelines.md`

Expected output:
- `step-4` fenced JSON findings

Hard stops:
- missing or malformed `step-3` JSON
- reviewer output missing or malformed
