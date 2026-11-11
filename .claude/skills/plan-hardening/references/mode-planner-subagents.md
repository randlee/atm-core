# Mode: planner with subagents

The planning agent runs steps 1–7 itself, with no teammates. Use it when the
team-mates are busy.

## Planner steps (1, 3, 5)

Render the step template with the vars file (sub-step 1) and do the work it
describes yourself. Skip its `atm task start`, `atm send` and `atm task close`
steps: there is no task. Close the step bead yourself (SKILL.md "Beads"). The fenced JSON the template specifies is your step
result; save it to `/tmp/step-<n>.json`.

## Reviewer steps (2, 4, 6)

Use the Agent tool to launch `.claude/agents/<reviewer>.md` with the step's
fenced JSON input. Set `run_in_background: true`. On the first loop round,
save the returned agent id. On subsequent loop rounds, re-use the same
reviewer agent if it is still available so review context carries forward.

A background reviewer generates no ATM traffic; after each reviewer round,
send the step doc's `plan-review-notice` to the phase lead.
