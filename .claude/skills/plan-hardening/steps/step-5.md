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

**3. Wait for response**

Wait for `arch-ctm` to return a message containing fenced JSON.
The expected output shape is specified inside
`03-consistency-hardening.xml.j2`.
Do not proceed to Step 6 until that fenced JSON is present and well formed.

## Hard stops

- `step-4` fenced JSON is missing or malformed: stop, report which field is
  missing
- `arch-ctm` reports unresolved architecture, boundary, or false-closure
  findings: stop, do not proceed
- fenced JSON is missing or malformed: stop, report which field is missing
