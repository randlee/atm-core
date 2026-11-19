# Step 7 — Focused Plan QA (`quality-mgr`)

## Execute

**1. Hand off to the plan gate**

Run the plan gate in `.claude/skills/atm-bd-orchestration/SKILL.md`
("Plan Gate"): it creates the plan-review bead `<root>-plan-qa`, refreshes the
phase diagram, runs `validate-plan` and dispatches `templates.plan_review` from
`.atm-bd/phase-<phase>.toml` to `quality-mgr`. Start the vars from
`.claude/skills/atm-bd-orchestration/examples/plan-review-template-vars.json`.

Use `step-6` fenced JSON only to confirm that the expected plan changes were
made. Do not invent QA scope from memory.

**2. Handoff**

After the QA task is sent, the plan gate owns reviewer execution, reporting,
fix routing and recheck rounds. Do not route QA findings back through local
`/plan-hardening` steps.

## Hard stops

- `validate-plan` does not exit 0: do not dispatch; fix the plan first
- an open `stage:plan-finding` bead for the phase: route it to Step 5 first
- `step-6` fenced JSON from the Step 6 response is missing or malformed: do
  not advance; send a correction request immediately and identify the missing
  or malformed fields explicitly
