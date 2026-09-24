# Step 6 — Focused Plan QA (`quality-mgr`)

## Execute

**1. Preview the message**

```bash
atm compose --template .claude/skills/codex-orchestration/qa-template.xml.j2 \
  --vars /tmp/plan-hardening-qa-vars.json
```

This is a preview only; the rendered text is never what gets sent.

The vars file or rendered task must include the QA assignment fields required
by `qa-template.xml.j2`, and it must use `step-5` fenced JSON to populate the
QA scope. Use `review_mode: "plan"`.

Expected `/tmp/plan-hardening-qa-vars.json` shape:

```json
{
  "task_id": "phase-bc-plan-qa",
  "sprint": "phase-bc",
  "sprint_doc": "docs/plans/phase-bc/phase-bc-plan.md",
  "review_mode": "plan",
  "description": "Focused plan QA for phase-bc after consistency hardening",
  "pr_number": "",
  "branch": "plan/phase-bc",
  "worktree_path": "/absolute/path/to/worktree",
  "commits": "HEAD",
  "review_targets": [
    "docs/plans/phase-bc/phase-bc-plan.md",
    "docs/plans/phase-bc/sprint-bc-1-<slug>.md",
    "docs/plans/phase-bc/sprint-bc-2-<slug>.md"
  ],
  "references": [
    "docs/project-plan.md"
  ],
  "changed_files": "",
  "triage_records": ""
}
```

Populate `sprint_doc` and `review_targets` by listing the phase plan and every
sprint doc in the current plan state. Use `step-5` fenced JSON only to confirm
that the expected files were modified or created. Do not invent QA scope from
memory.

**2. Send to `quality-mgr`**

Assign the task to `quality-mgr` with the template, so the assignment is
queued, nudged and tracked:

```bash
VARS=/tmp/plan-hardening-qa-vars.json
atm task assign quality-mgr --task-id "$(jq -r .task_id "$VARS")" \
  --template .claude/skills/codex-orchestration/qa-template.xml.j2 \
  --vars "$VARS"
```

Never send a rendered file with `--file` or `--stdin`: it drops the template's
workflow metadata, so the QA round cannot be found or counted afterwards.

**3. Handoff**

After the QA task is sent, follow the codex-orchestration plan review flow.
Do not route QA findings back through local `/plan-hardening` steps. From this
point forward, the QA system owns reviewer execution, reporting, fix routing,
and recheck loops.

## Hard stops

- `/tmp/plan-hardening-qa-vars.json` is missing required QA assignment fields:
  do not advance; correct the QA vars file immediately
- `step-5` fenced JSON from the Step 5 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
