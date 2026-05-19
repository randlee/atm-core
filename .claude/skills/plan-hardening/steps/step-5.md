# Step 5 — Consistency Hardening (`arch-ctm`)

## Execute

**1. Render the message**

```bash
sc-compose render \
  --root .claude/skills/plan-hardening \
  --file 03-consistency-hardening.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json \
  --output /tmp/step-5-message.xml
```

The vars file or rendered task must include `step-4` fenced JSON as the
required input payload.

**2. Send to `arch-ctm`**

```bash
atm send arch-ctm --stdin < /tmp/step-5-message.xml
```

**3. Check the response**

Read the `arch-ctm` response and confirm it contains fenced JSON.
The expected output shape is specified inside
`03-consistency-hardening.xml.j2`.
Do not proceed to Step 6 until that fenced JSON is present and well formed.
If the response is incomplete or malformed, send a correction request to
`arch-ctm` immediately.

## Hard stops

- `step-4` fenced JSON from the Step 4 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- `arch-ctm` reports unresolved architecture, boundary, or false-closure
  findings: do not advance; route corrective action back through `arch-ctm`
- fenced JSON is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
