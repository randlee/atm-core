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
- QA verdict is fail: do not advance; route the findings back through
  hardening
