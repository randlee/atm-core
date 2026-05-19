# Step 5

Route to: `arch-ctm`

Purpose:
- resolve the `critical-plan-reviewer` findings
- eliminate contradiction, ambiguity, and missing ADR/boundary coverage

Action:
- render `03-consistency-hardening.xml.j2` with `sc-compose`
- pass `step-4` fenced JSON as required input
- send to `arch-ctm`
- wait for `step-5` fenced JSON

Required input:
- `step-4` fenced JSON

Expected output:
- `step-5` fenced JSON from consistency hardening

Hard stops:
- missing or malformed `step-4` JSON
- unresolved architecture, boundary, or false-closure findings
