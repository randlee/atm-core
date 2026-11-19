# Mode: lead runs hardening adversarially to the planner

The phase lead runs steps 1–7. The planner is a teammate who edits the plan;
the reviewers run as background agents the lead launches. The lead routes the
process but is not the authority for rewriting the plan.

## Planner steps (1, 3, 5)

```bash
VARS=/tmp/plan-hardening-vars.json
AGENT=<planner>                        # the plan author chosen by the lead
TASK_ID="$(jq -r .task_id "$VARS")"     # the step bead id
atm task assign "$AGENT" --task-id "$TASK_ID" \
  --template .claude/skills/plan-hardening/<step template> --vars "$VARS"
```

The rendered preview from sub-step 1 is never what gets sent. Send the
template, not the rendered text: a plain `atm send --stdin` opens no task, so
the planner's `atm task start` in the template's first step would fail. The
agent is named only on this command; the vars file and the message body carry
no assignee. Use the same agent for steps 1, 3 and 5 of one hardening run.

## Reviewer steps (2, 4, 6)

Use the Agent tool to launch `.claude/agents/<reviewer>.md` with the step's
fenced JSON input. Set `run_in_background: true`. On the first loop round,
save the returned agent id. On subsequent loop rounds, re-use the same
reviewer agent if it is still available so review context carries forward.

A background reviewer generates no ATM traffic; the step doc's "Record the
round in ATM" notice goes to the planner with `atm queue`, so it never
interrupts work in progress.
