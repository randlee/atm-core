# Mode: planner with teammates

The planning agent runs steps 1–7 itself; the reviewers are teammates.

## Planner steps (1, 3, 5)

Render the step template with the vars file (sub-step 1) and do the work it
describes yourself. Skip its `atm task start`, `atm send` and `atm task close`
steps: there is no task. Close the step bead yourself (SKILL.md "Beads"). The fenced JSON the template specifies is your step
result; save it to `/tmp/step-<n>.json`.

## Reviewer steps (2, 4, 6)

Assign the review to a teammate with
`.claude/skills/plan-hardening/plan-critical-review.xml.j2`, so the review is
queued, nudged and tracked:

```bash
atm task assign <reviewer teammate> --task-id <step bead id> \
  --template .claude/skills/plan-hardening/plan-critical-review.xml.j2 \
  --vars <vars>
```

Set `reviewer` in the vars to the step's reviewer (`plan-scope-reviewer`,
`critical-plan-reviewer` or `ceremony-qa`); for `plan-scope-reviewer` also set
`complete_template` to `plan-scope-review-complete.md.j2`. Put the step's
fenced JSON input in `previous_step_json`. The teammate's task close carries
the reviewer's fenced JSON in `result_json`; that is the step response.
The task assignment records the round, so no
`plan-review-notice` is sent.
