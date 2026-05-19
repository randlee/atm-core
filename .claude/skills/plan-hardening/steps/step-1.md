# Step 1

Route to: `arch-ctm`

Purpose:
- read the sprint-planning guidelines
- make sure the current plan state follows them before reviewer launch

Action:
- render `01-plan-scope-review.xml.j2` with `sc-compose`
- send it to `arch-ctm`
- wait for `step-1` fenced JSON

Required input:
- vars file
- current planning docs
- `.claude/skills/plan-hardening/sprint-planning-guidelines.md`

Expected output:
- `step-1` fenced JSON from the initial guidelines pass

Hard stops:
- missing worktree
- scope conflict with the user-discussed plan
- missing or malformed `step-1` JSON
