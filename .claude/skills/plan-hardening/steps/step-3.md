# Step 3 — Sprint Scope Hardening (developer)

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

**2. Send to the developer**

```bash
VARS=/tmp/plan-hardening-vars.json
AGENT=<developer>                      # the plan author chosen by the lead
TASK_ID="$(jq -r .task_id "$VARS")"
atm task assign "$AGENT" --task-id "$TASK_ID" \
  --template .claude/skills/plan-hardening/02-sprint-scope-hardening.xml.j2 --vars "$VARS"
```

The rendered file from sub-step 1 is a preview only. Send the template, not
the rendered text: a plain `atm send --stdin` opens no task, so the
developer's `atm task start` in the template's first step would fail. The
agent is named only on this command; the vars file and the message body carry
no assignee. Use the same agent for steps 1, 3 and 5 of one hardening run.

**3. Check the response**

Read the developer's response and confirm it contains fenced JSON.
The expected output shape is specified inside
`02-sprint-scope-hardening.xml.j2`.
Do not proceed to Step 4 until that fenced JSON is present and well formed.
If the response is incomplete or malformed, send a correction request to
the developer immediately.
Save the extracted fenced JSON to `/tmp/step-3.json`.

**4. Route by status**

- `PASS` -> proceed to Step 4
- `FAIL` -> re-render and re-send Step 3 to the developer
- if the developer ACKs but responds as though the same already-fixed round is
  being replayed, increment `round_index`, update `round_id`, refresh
  `replay_nonce` with the current UTC timestamp, and re-render before
  re-sending

Maintain the round table after every Step 3 / Step 4 loop:

| Round | Step | Reviewer | reviewed_commit | status | blocking | important | minor | findings_hash | supersedes | Note |
|-------|------|----------|-----------------|--------|----------|-----------|-------|---------------|------------|------|

## Hard stops

- `step-2` fenced JSON from the Step 2 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
- fenced JSON is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
