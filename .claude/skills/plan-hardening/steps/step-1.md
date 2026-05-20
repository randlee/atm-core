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

If `/tmp/plan-hardening-vars.json` does not exist, start from:

`.claude/skills/plan-hardening/examples/plan-hardening-vars.example.json`

Make sure the vars file includes the current round metadata:
- `round_id`
- `round_index`
- `replay_nonce`
- `reviewed_commit`
- `previous_reviewed_commit`
- `findings_hash`

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
Save the extracted fenced JSON to `/tmp/step-1.json`.

**4. Route by status**

- `PASS` -> proceed to Step 2
- `FAIL` -> re-render and re-send Step 1 to `arch-ctm`
- if `arch-ctm` ACKs but returns no new fenced JSON, increment `round_index`,
  update `round_id`, refresh `replay_nonce` with the current UTC timestamp,
  and re-render Step 1 before re-sending

## Hard stops

- worktree does not exist: create it before running this step
- fenced JSON is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
