# Step 2 — Scope Review (`plan-scope-reviewer`, background)

## Execute

**1. Launch the reviewer**

Use Agent tool to launch `.claude/agents/plan-scope-reviewer.md`.
Pass a fenced JSON input that includes:
- `source_of_truth`
- `references`
- `worktree_path`
- `branch`
- `step-1` fenced JSON

Set `run_in_background: true`.

Expected reviewer launch input shape:

```json
{
  "source_of_truth": "docs/phase-X/plan-phase-X.md",
  "references": [
    "docs/project-plan.md"
  ],
  "worktree_path": "/absolute/path/to/worktree",
  "branch": "feature/branch-name",
  "previous_step_json": {
    "status": "PASS",
    "mode": "plan-hardening-guidelines-pass"
  }
}
```

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
- `FAIL` -> update `/tmp/plan-hardening-vars.json` so
  `reviewer_findings_json` contains the Step 2 fenced JSON, then re-run Step 1

## Hard stops

- `step-1` fenced JSON from the Step 1 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- reviewer launch input is missing `source_of_truth`, `references`,
  `worktree_path`, `branch`, or `step-1` fenced JSON: do not advance; correct
  the launch payload immediately
- reviewer output is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
