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
QA scope. Use `review_mode: "plan"`.

Expected `/tmp/plan-hardening-qa-vars.json` shape:

```json
{
  "task_id": "phase-x-plan-qa",
  "sprint": "phase-X",
  "sprint_doc": "docs/phase-X/plan-phase-X.md",
  "review_mode": "plan",
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

Populate `sprint_doc` and `review_targets` by listing the phase plan and every
sprint doc in the current plan state. Use `step-5` fenced JSON only to confirm
that the expected files were modified or created. Do not invent QA scope from
memory.

**2. Send to `quality-mgr`**

Use the SendMessage tool to send the rendered XML content from
`/tmp/step-6-message.xml` to the named teammate `quality-mgr`.
Do not use `atm send` for this step.

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
