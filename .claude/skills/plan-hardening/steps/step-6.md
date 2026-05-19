# Step 6 — Focused Plan QA (`quality-mgr`)

## Execute

**1. Render the message**

```bash
sc-compose render \
  --root .claude/skills/codex-orchestration \
  --file qa-template.xml.j2 \
  --var-file /tmp/plan-hardening-qa-vars.json \
  --output /tmp/step-6-message.xml
```

The vars file or rendered task must include the QA assignment fields required
by `qa-template.xml.j2`, and it must use `step-5` fenced JSON to populate the
QA scope.

Expected `/tmp/plan-hardening-qa-vars.json` shape:

```json
{
  "task_id": "phase-x-plan-qa",
  "sprint": "phase-X",
  "sprint_doc": "docs/phase-X/plan-phase-X.md",
  "review_mode": "plan_hardening",
  "description": "Focused plan QA for phase-X after consistency hardening",
  "pr_number": "",
  "branch": "feature/branch-name",
  "worktree_path": "/absolute/path/to/worktree",
  "commits": "HEAD",
  "review_targets": [
    "docs/phase-X/plan-phase-X.md",
    "docs/phase-X/sprint-X1.md",
    "docs/phase-X/sprint-X2.md"
  ],
  "references": [
    "docs/project-plan.md"
  ],
  "changed_files": "",
  "triage_records": ""
}
```

Populate `sprint_doc`, `review_targets`, and `references` from the current
plan state and `step-5` fenced JSON. Do not invent QA scope from memory.

**2. Send to `quality-mgr`**

```bash
atm send quality-mgr --stdin < /tmp/step-6-message.xml
```

**3. Check the response**

Read the `quality-mgr` response and confirm it returns a fenced JSON
machine-status block with a top-level `status`.
Do not treat the plan as implementation-ready until that top-level status is
`PASS`. If the response is incomplete or malformed, send a correction request
to `quality-mgr` immediately.

**4. Route by status**

- `PASS` -> plan hardening is complete
- `FAIL` -> update `/tmp/plan-hardening-vars.json` so
  `reviewer_findings_json` contains the QA findings JSON, then re-run Step 5

## Hard stops

- `/tmp/plan-hardening-qa-vars.json` is missing required QA assignment fields:
  do not advance; correct the QA vars file immediately
- `step-5` fenced JSON from the Step 5 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- top-level QA `status` is `FAIL`: do not advance; route the findings back
  through hardening
