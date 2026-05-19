# Step 2 — Scope Review (`plan-scope-reviewer`, background)

## Execute

**1. Launch the reviewer**

Use Agent tool to launch `.claude/agents/plan-scope-reviewer.md`.
On each subsequent loop round, launch a fresh unnamed background agent with
the updated vars file after `reviewer_findings_json` has been populated from
the previous round output.
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
Save the extracted fenced JSON to `/tmp/step-2.json`.

**3. Route by status**

- `PASS` -> proceed to Step 3
- `FAIL` -> update `/tmp/plan-hardening-vars.json` so
  `reviewer_findings_json` contains the Step 2 fenced JSON, then re-run Step 1
- after Step 1 returns updated fenced JSON, launch a fresh unnamed background
  `plan-scope-reviewer` agent with the updated vars file

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

## Hard stops

- `step-1` fenced JSON from the Step 1 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- reviewer launch input is missing `source_of_truth`, `references`,
  `worktree_path`, `branch`, or `step-1` fenced JSON: do not advance; correct
  the launch payload immediately
- reviewer output is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
- reviewer has returned `FAIL` three times without converging: do not advance;
  escalate to the user before continuing
