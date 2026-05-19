# Step 3 — Sprint Scope Hardening (`arch-ctm`)

## Execute

**1. Render the message**

```bash
sc-compose render \
  --root .claude/skills/plan-hardening \
  --file 02-sprint-scope-hardening.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json \
  --output /tmp/step-3-message.xml
```

The vars file or rendered task must include `step-2` fenced JSON as the
required input payload.

**2. Send to `arch-ctm`**

```bash
atm send arch-ctm /tmp/step-3-message.xml
```

**3. Wait for response**

Wait for `arch-ctm` to return a message containing fenced JSON.
The expected output shape is specified inside
`02-sprint-scope-hardening.xml.j2`.
Do not proceed to Step 4 until that fenced JSON is present and well formed.

## Hard stops

- `step-2` fenced JSON is missing or malformed: stop, report which field is
  missing
- `arch-ctm` reports unresolved split-risk, drop-risk, or sprint-ownership
  gaps: stop, do not proceed
- fenced JSON is missing or malformed: stop, report which field is missing
