# Step 6 — Ceremony Review (`ceremony-qa`)

Too much critical review can add ceremony, so `ceremony-qa` reviews the plan
after the final critical review and its Step 5 correction pass.

## Execute

**1. Dispatch the reviewer**

Import the step bead (SKILL.md "Beads", `step` 6, `route_to` `ceremony-qa`). Dispatch `ceremony-qa` as your mode reference directs (SKILL.md "Modes").
Pass a fenced JSON input that includes:
- `review_mode`: `doc_review`
- `worktree_path`
- `review_targets`: the plan file `docs/plans/phase-<phase>.jsonl`
- `carry_forward_findings`: `[]`

**2. Check the response**

Read the `ceremony-qa` response and confirm it returns fenced JSON findings.
The expected output shape is specified inside `.claude/agents/ceremony-qa.md`.
Do not proceed to Step 7 until that fenced JSON is present and well formed.
If the response is incomplete or malformed, send a correction request to
`ceremony-qa` immediately.
Save the extracted fenced JSON to `/tmp/step-6.json`.

**3. Route by status**

- `PASS` -> proceed to Step 7
- `FAIL` -> update `/tmp/plan-hardening-vars.json` so
  `reviewer_findings_json` contains the Step 6 fenced JSON, then re-run Step 5
  once; after that correction pass, proceed to Step 7

Record the round in beads after every Step 6 response (SKILL.md "Beads"):
file one finding bead per finding (`reviewer` `ceremony-qa`), then close the Step 6
step bead with the status, counts, `reviewed_commit` and `findings_hash`.

## Hard stops

- reviewer output is missing or malformed: do not advance; send a correction
  request immediately and identify the missing or malformed fields explicitly
