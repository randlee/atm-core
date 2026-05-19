# Step 3

Route to: `arch-ctm`

Purpose:
- harden sprint shape from the `plan-scope-reviewer` findings
- create missing sprint docs
- split overloaded sprints

Action:
- render `02-sprint-scope-hardening.xml.j2` with `sc-compose`
- pass `step-2` fenced JSON as required input
- send to `arch-ctm`
- wait for `step-3` fenced JSON

Required input:
- `step-2` fenced JSON

Expected output:
- `step-3` fenced JSON from sprint-scope hardening

Hard stops:
- missing or malformed `step-2` JSON
- unresolved split-risk, drop-risk, or ownership gaps
