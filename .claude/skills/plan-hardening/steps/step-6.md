# Step 6 — Focused Plan QA (`quality-mgr`)

## Execute

**1. Launch QA**

Use Agent tool to launch `.claude/agents/quality-mgr.md`.
Pass `step-5` fenced JSON as required input.

**2. Check the response**

Read the `quality-mgr` response and confirm it returns a fenced JSON
machine-status block with a top-level `status`.
Do not treat the plan as implementation-ready until that top-level status is
`PASS`. If the response is incomplete or malformed, send a correction request
to `quality-mgr` immediately.

**3. Route by status**

- `PASS` -> plan hardening is complete
- `FAIL` -> update `/tmp/plan-hardening-vars.json` so
  `reviewer_findings_json` contains the QA findings JSON, then re-run Step 5

## Hard stops

- `step-5` fenced JSON from the Step 5 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- QA verdict is fail: do not advance; route the findings back through
  hardening
