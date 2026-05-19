# Step 2 — Scope Review (`plan-scope-reviewer`, background)

## Execute

**1. Launch the reviewer**

Use Agent tool to launch `.claude/agents/plan-scope-reviewer.md`.
Pass `step-1` fenced JSON as required input and set
`run_in_background: true`.

**2. Wait for response**

Wait for `plan-scope-reviewer` to return fenced JSON findings.
The expected output shape is specified inside
`.claude/agents/plan-scope-reviewer.md`.
Do not proceed to Step 3 until that fenced JSON is present and well formed.

## Hard stops

- `step-1` fenced JSON is missing or malformed: stop, report which field is
  missing
- reviewer output is missing or malformed: stop, report which field is missing
