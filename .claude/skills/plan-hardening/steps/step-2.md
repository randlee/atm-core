# Step 2 — Scope Review (`plan-scope-reviewer`, background)

## Execute

**1. Launch the reviewer**

Use Agent tool to launch `.claude/agents/plan-scope-reviewer.md`.
Pass `step-1` fenced JSON as required input and set
`run_in_background: true`.

**2. Check the response**

Read the `plan-scope-reviewer` response and confirm it returns fenced JSON
findings.
The expected output shape is specified inside
`.claude/agents/plan-scope-reviewer.md`.
Do not proceed to Step 3 until that fenced JSON is present and well formed.
If the response is incomplete or malformed, send a correction request to
`plan-scope-reviewer` immediately.

**3. Route by status**

- `PASS` -> proceed to Step 3
- `FAIL` -> re-run Step 1 and include the Step 2 fenced JSON as reviewer
  findings for `arch-ctm`

## Hard stops

- `step-1` fenced JSON from the Step 1 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- reviewer output is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
