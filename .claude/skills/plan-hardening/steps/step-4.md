# Step 4 — Critical Plan Review (`critical-plan-reviewer`, background)

## Execute

**1. Launch the reviewer**

Use Agent tool to launch `.claude/agents/critical-plan-reviewer.md`.
On the first loop round, save the returned agent id. On subsequent loop
rounds, re-use the same reviewer agent if it is still available so review
context carries forward.
Pass a fenced JSON input that includes:
- `source_of_truth`
- `references`
- `worktree_path`
- `branch`
- `step-3` fenced JSON

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
  "reviewed_commit": "abc1234",
  "previous_reviewed_commit": "",
  "findings_hash": "",
  "previous_step_json": {
    "status": "PASS",
    "mode": "plan-hardening-sprint-scope",
    "round_id": "STEP3-R1",
    "round_index": 1,
    "reviewed_commit": "abc1234",
    "previous_reviewed_commit": ""
  }
}
```

**2. Check the response**

Read the `critical-plan-reviewer` response and confirm it returns fenced JSON
findings.
The expected output shape is specified inside
`.claude/agents/critical-plan-reviewer.md`.
Do not proceed to Step 5 until that fenced JSON is present and well formed.
If the response is incomplete or malformed, send a correction request to
`critical-plan-reviewer` immediately.
Save the extracted fenced JSON to `/tmp/step-4.json`.

**3. Route by status**

- `PASS` -> proceed to Step 5
- `FAIL` -> update `/tmp/plan-hardening-vars.json` so
  `reviewer_findings_json` contains the Step 4 fenced JSON, then re-run Step 3
- after Step 3 returns updated fenced JSON, update:
  - `previous_reviewed_commit`
  - `reviewed_commit`
  - `findings_hash`
  - `supersedes_task_id`
  - `replay_nonce`
  then send the updated payload back to the same
  `critical-plan-reviewer` agent when possible
- the reviewer must return all remaining `Blocking` and `Important` findings
  in one pass; newly surfaced findings after a previous round are acceptable
  only if the plan changed between rounds
- if the next Step 4 response repeats the same `reviewed_commit` and the same
  `findings_hash`, classify it as a stale replay and do not open a new Step 3
  round

Example reinjection command:

```bash
python3 - <<'PY'
import json
from pathlib import Path
vars_path = Path('/tmp/plan-hardening-vars.json')
data = json.loads(vars_path.read_text())
data['reviewer_findings_json'] = Path('/tmp/step-4.json').read_text()
vars_path.write_text(json.dumps(data, indent=2) + '\\n')
PY
```

The reviewer output schema includes:
- `findings` for structural `Blocking` / `Important` issues
- `minor_wording` for wording-only cleanup
- `findings_hash` as the stable round fingerprint

Update the round table after every Step 4 response:

| Round | Step | Reviewer | reviewed_commit | status | blocking | important | minor | findings_hash | supersedes | Note |
|-------|------|----------|-----------------|--------|----------|-----------|-------|---------------|------------|------|

## Hard stops

- `step-3` fenced JSON from the Step 3 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- reviewer launch input is missing `source_of_truth`, `references`,
  `worktree_path`, `branch`, or `step-3` fenced JSON: do not advance; correct
  the launch payload immediately
- reviewer output is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
- reviewer output repeats the same `reviewed_commit` and the same
  `findings_hash`: do not advance; mark it as stale replay and request a fresh
  review cycle only after the plan state changes
- reviewer has returned `FAIL` three times without converging: do not advance;
  escalate to the user before continuing
