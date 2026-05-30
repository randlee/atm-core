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
It must also carry current round metadata:
- `round_id`
- `round_index`
- `replay_nonce`
- `reviewed_commit`
- `previous_reviewed_commit`
- `findings_hash`

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
Save the extracted fenced JSON to `/tmp/step-5.json`.

**4. Route by status**

- `PASS` -> proceed to Step 6
- `FAIL` -> re-render and re-send Step 5 to `arch-ctm`
- if `arch-ctm` ACKs but responds as though the same already-fixed round is
  being replayed, increment `round_index`, update `round_id`, refresh
  `replay_nonce` with the current UTC timestamp, and re-render before
  re-sending
- if Step 5 returns `FAIL` three times without converging, stop and report
  `cap-exhausted / not converged`; do not ask the user what to do mid-loop

## Hard stops

- `step-4` fenced JSON from the Step 4 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- fenced JSON is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
- Step 5 has returned `FAIL` three times without converging: do not advance;
  stop and report `cap-exhausted / not converged` without prompting the user
