# Step 1 — Plan Scope Review (`arch-ctm`)

## Execute

**1. Render the message**

```bash
sc-compose render \
  --root .claude/skills/plan-hardening \
  --file 01-plan-scope-review.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json \
  --output /tmp/step-1-message.xml
```

**2. Send to `arch-ctm`**

```bash
atm send arch-ctm --stdin < /tmp/step-1-message.xml
```

**3. Check the response**

Read the `arch-ctm` response and confirm it contains fenced JSON.
The expected output shape is specified inside `01-plan-scope-review.xml.j2`.
Do not proceed to Step 2 until that fenced JSON is present and well formed.
If the response is incomplete or malformed, send a correction request to
`arch-ctm` immediately.

## Hard stops

- worktree does not exist: create it before running this step
- `arch-ctm` ACK or first substantive response indicates a material scope
  change from the user-discussed plan: do not advance; send a clarification
  request to `arch-ctm` first, and escalate to the user only if this is a real
  scope dispute
- fenced JSON is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
