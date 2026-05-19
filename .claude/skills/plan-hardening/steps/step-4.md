# Step 4 — Critical Plan Review (`critical-plan-reviewer`, background)

## Execute

**1. Launch the reviewer**

Use Agent tool to launch `.claude/agents/critical-plan-reviewer.md`.
Pass a fenced JSON input that includes:
- `source_of_truth`
- `references`
- `worktree_path`
- `branch`
- `step-3` fenced JSON

Set `run_in_background: true`.

**2. Check the response**

Read the `critical-plan-reviewer` response and confirm it returns fenced JSON
findings.
The expected output shape is specified inside
`.claude/agents/critical-plan-reviewer.md`.
Do not proceed to Step 5 until that fenced JSON is present and well formed.
If the response is incomplete or malformed, send a correction request to
`critical-plan-reviewer` immediately.

**3. Route by status**

- `PASS` -> proceed to Step 5
- `FAIL` -> update `/tmp/plan-hardening-vars.json` so
  `reviewer_findings_json` contains the Step 4 fenced JSON, then re-run Step 3

## Hard stops

- `step-3` fenced JSON from the Step 3 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- reviewer launch input is missing `source_of_truth`, `references`,
  `worktree_path`, `branch`, or `step-3` fenced JSON: do not advance; correct
  the launch payload immediately
- reviewer output is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
