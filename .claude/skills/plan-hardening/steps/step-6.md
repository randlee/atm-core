# Step 6

Route to: `quality-mgr`

Purpose:
- final focused plan QA after both reviewer passes and both hardening passes

Action:
- route the hardened plan to `quality-mgr`
- pass `step-5` fenced JSON as required input
- wait for QA verdict

Required input:
- `step-5` fenced JSON

Expected output:
- QA verdict

Hard stops:
- missing or malformed `step-5` JSON
- QA fail
