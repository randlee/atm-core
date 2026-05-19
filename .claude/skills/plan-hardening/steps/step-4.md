# Step 4 — Critical Plan Review (`critical-plan-reviewer`, background)

## Execute

**1. Launch the reviewer**

Use Agent tool to launch `.claude/agents/critical-plan-reviewer.md`.
Pass `step-3` fenced JSON as required input and set `run_in_background: true`.

**2. Wait for response**

Wait for `critical-plan-reviewer` to return fenced JSON findings.
The expected output shape is specified inside
`.claude/agents/critical-plan-reviewer.md`.
Do not proceed to Step 5 until that fenced JSON is present and well formed.

## Hard stops

- `step-3` fenced JSON is missing or malformed: stop, report which field is
  missing
- reviewer output is missing or malformed: stop, report which field is missing
