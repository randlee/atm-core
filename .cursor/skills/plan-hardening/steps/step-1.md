# Step 1 — Plan Scope Review (planner)

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

**2. Dispatch the planner step**

Import the step bead (SKILL.md "Beads", `step` 1, `route_to` the planner) and set
`task_id` in the vars file to its id. Dispatch `01-plan-scope-review.xml.j2` with the vars file as your mode reference directs
(SKILL.md "Modes"). The same planner runs steps 1, 3 and 5 of one hardening
run.

**3. Check the response**

Read the planner's response and confirm it contains fenced JSON.
The expected output shape is specified inside `01-plan-scope-review.xml.j2`.
Do not proceed to Step 2 until that fenced JSON is present and well formed.
If the response is incomplete or malformed, send a correction request to
the planner immediately.
Save the extracted fenced JSON to `/tmp/step-1.json`.

**4. Route by status**

- `PASS` -> proceed to Step 2
- `FAIL` -> re-render and re-send Step 1 to the planner
- if the planner ACKs but returns no new fenced JSON, increment `round_index`,
  update `round_id`, refresh `replay_nonce` with the current UTC timestamp,
  and re-render Step 1 before re-sending

## Hard stops

- worktree does not exist: create it before running this step
- fenced JSON is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
