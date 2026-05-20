# Step 3 — Sprint Scope Hardening (`arch-ctm`)

## Stale Replay Detection

A new step-3 task is valid only when **at least one** of the following is true:

- `reviewed_commit` differs from `previous_reviewed_commit`, OR
- `findings_hash` of the new step-4 JSON differs from the prior round's `findings_hash`

If neither condition is met, do not dispatch. Update `round_id` (increment suffix) and set `replay_nonce` to the current UTC timestamp before re-rendering.

**replay_nonce enforcement**: `replay_nonce` MUST be set to the current UTC timestamp (e.g., `2026-05-20T06:19:00Z`) before every render. A stale or repeated `replay_nonce` value silently defeats the anti-replay guarantee. Never reuse a `replay_nonce` between rounds.

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
It must also carry current round metadata:
- `round_id`
- `round_index`
- `replay_nonce`
- `reviewed_commit`
- `previous_reviewed_commit`
- `findings_hash`

**2. Send to `arch-ctm`**

```bash
atm send arch-ctm --stdin < /tmp/step-3-message.xml
```

**3. Check the response**

Read the `arch-ctm` response and confirm it contains fenced JSON.
The expected output shape is specified inside
`02-sprint-scope-hardening.xml.j2`.
Do not proceed to Step 4 until that fenced JSON is present and well formed.
If the response is incomplete or malformed, send a correction request to
`arch-ctm` immediately.
Save the extracted fenced JSON to `/tmp/step-3.json`.

**4. Route by status**

- `PASS` -> proceed to Step 4
- `FAIL` -> re-render and re-send Step 3 to `arch-ctm`
- if `arch-ctm` ACKs but responds as though the same already-fixed round is
  being replayed, increment `round_index`, update `round_id`, refresh
  `replay_nonce` with the current UTC timestamp, and re-render before
  re-sending

Maintain the round table after every Step 3 / Step 4 loop:

| Round | Step | Reviewer | reviewed_commit | status | blocking | important | minor | findings_hash | supersedes | Note |
|-------|------|----------|-----------------|--------|----------|-----------|-------|---------------|------------|------|

**Non-convergence escalation**: If round N has `Total >= round N-2` (i.e., total finding count has not decreased over two rounds), team-lead **must escalate to the user** before dispatching round N+1.

## Hard stops

- `step-2` fenced JSON from the Step 2 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- fenced JSON is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
