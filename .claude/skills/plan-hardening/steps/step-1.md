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
atm send arch-ctm /tmp/step-1-message.xml
```

**3. Wait for response**

Wait for `arch-ctm` to return a message containing fenced JSON.
The expected output shape is specified inside `01-plan-scope-review.xml.j2`.
Do not proceed to Step 2 until that fenced JSON is present and well formed.

## Hard stops

- worktree does not exist: create it before running this step
- `arch-ctm` ACK describes a material scope change from the user-discussed
  plan: stop, report conflict to the user, do not proceed
- fenced JSON is missing or malformed: stop, report which field is missing
