# Step 6 — Focused Plan QA (`quality-mgr`)

## Execute

**1. Launch QA**

Use Agent tool to launch `quality-mgr`.
Pass `step-5` fenced JSON as required input.

**2. Wait for response**

Wait for `quality-mgr` to return the QA verdict.
Do not treat the plan as implementation-ready until the verdict is a pass.

## Hard stops

- `step-5` fenced JSON is missing or malformed: stop, report which field is
  missing
- QA verdict is fail: stop and route the findings back through hardening
