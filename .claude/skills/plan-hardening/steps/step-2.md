# Step 2 — Scope Review (`plan-scope-reviewer`, background)

## Execute

**1. Launch the reviewer**

Use Agent tool to launch `.claude/agents/plan-scope-reviewer.md`.
On the first loop round, save the returned agent id. On subsequent loop
rounds, re-use the same reviewer agent if it is still available so review
context carries forward.
Pass a fenced JSON input that includes:
- `source_of_truth`
- `references`
- `worktree_path`
- `branch`
- `review_cycle_limit`
- `review_cycle_index`
- `step-1` fenced JSON

Set `run_in_background: true`.

Expected reviewer launch input shape:

```json
{
  "source_of_truth": "docs/plans/phase-X/plan-phase-X.md",
  "references": [
    "docs/project-plan.md"
  ],
  "worktree_path": "/absolute/path/to/worktree",
  "branch": "feature/branch-name",
  "review_cycle_limit": 3,
  "review_cycle_index": 1,
  "reviewed_commit": "abc1234",
  "previous_reviewed_commit": "",
  "findings_hash": "",
  "previous_step_json": {
    "status": "PASS",
    "mode": "plan-hardening-guidelines-pass",
    "round_id": "STEP1-R1",
    "round_index": 1,
    "reviewed_commit": "abc1234",
    "previous_reviewed_commit": ""
  }
}
```

Determine `plan_scope_review_cycle_limit` from
`/tmp/plan-hardening-vars.json`. If it is missing, set it to `3` before
launching the reviewer. `review_cycle_index` is the count of completed Step 2
review responses in the current hardening run, starting at `1`.

**2. Check the response**

Read the `plan-scope-reviewer` response and confirm it returns fenced JSON
findings.
The expected output shape is specified inside
`.claude/agents/plan-scope-reviewer.md`.
Do not proceed to Step 3 until that fenced JSON is present and well formed.
If the response is incomplete or malformed, send a correction request to
`plan-scope-reviewer` immediately.
Save the extracted fenced JSON to `/tmp/step-2.json`.

**3. Route by status**

- `PASS` -> proceed to Step 3
- `FAIL` -> update `/tmp/plan-hardening-vars.json` so
  `reviewer_findings_json` contains the Step 2 fenced JSON, then re-run Step 1
- every Step 2 `FAIL` must be routed to Step 1; there is no accept-and-proceed
  path
- after Step 1 returns updated fenced JSON, update:
  - `previous_reviewed_commit`
  - `reviewed_commit`
  - `findings_hash`
  - `supersedes_task_id`
  - `replay_nonce`
  then:
  - if the just-completed reviewer response used a cycle index lower than
    `plan_scope_review_cycle_limit`, send the updated payload back to the same
    `plan-scope-reviewer` agent when possible
  - if the just-completed reviewer response used cycle index equal to
    `plan_scope_review_cycle_limit`, do not launch another background review;
    stop the hardening run after the Step 1 correction pass and report
    `cap-exhausted / not converged`
- if the next Step 2 response repeats the same `reviewed_commit` and the same
  `findings_hash`, classify it as a stale replay and do not open a new Step 1
  round

Example reinjection command:

```bash
python3 - <<'PY'
import json
from pathlib import Path
vars_path = Path('/tmp/plan-hardening-vars.json')
data = json.loads(vars_path.read_text())
data['reviewer_findings_json'] = Path('/tmp/step-2.json').read_text()
vars_path.write_text(json.dumps(data, indent=2) + '\\n')
PY
```

Update the round table after every Step 2 response:

| Round | Step | Reviewer | reviewed_commit | status | blocking | important | minor | findings_hash | supersedes | Note |
|-------|------|----------|-----------------|--------|----------|-----------|-------|---------------|------------|------|

## Hard stops

- `step-1` fenced JSON from the Step 1 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- reviewer launch input is missing `source_of_truth`, `references`,
  `worktree_path`, `branch`, `review_cycle_limit`, `review_cycle_index`, or
  `step-1` fenced JSON: do not advance; correct the launch payload immediately
- reviewer output is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
- reviewer output repeats the same `reviewed_commit` and the same
  `findings_hash`: do not advance; mark it as stale replay and request a fresh
  review cycle only after the plan state changes
- reviewer has reached `plan_scope_review_cycle_limit` without converging: do
  not launch another reviewer cycle, do not ask the user what to do, and do
  not accept the findings silently; finish the Step 1 correction pass and
  report `cap-exhausted / not converged`
