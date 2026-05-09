---
name: plan-hardening
version: 1.0.0
description: >
  Team-lead delegates plan hardening to arch-ctm after the user has already
  discussed the plan details with arch-ctm.
depends_on:
  codex-orchestration: 0.x
---

# Plan Hardening

Audience: `team-lead` only.

Use this only for phase-plan hardening before implementation starts or resumes.

If the user invokes this skill, that means that the plan details have already
been discussed and are fresh in arch-ctm context. Do not request details from
the user, the details will surface when the plan is delivered.

`team-lead` is responsible for routing, worktree creation, and assignment
metadata. `team-lead` is not the authority for rewriting the plan.

## Preconditions

- the target phase worktree already exists
- `worktree_path` and `branch` are known
- `sc-compose` is available
- the plan has already been discussed with `arch-ctm`

## Expected Result

The task must end with:
- complete/consistent planning docs
- a hardened sprint doc for every sprint still required to finish the phase
- no unassigned in-scope implementation work
- branch pushed, validation reported, and plan review requested

Any remaining in-scope work without sprint ownership is a `GAP`. If more
sprints are needed, hardening must create them.

## Team-Lead Steps

1. Prepare:
   - `phase_id`
   - `task_id`
   - `description`
   - `worktree_path`
   - `branch`
   - `pr_target`
   - `source_of_truth`
   - optional `questions_or_concerns`
   - `references`
2. Render `.claude/skills/plan-hardening/plan-hardening.xml.j2` with
   `sc-compose`.
3. Send the rendered ATM task to `arch-ctm`.
4. Review the result:
   - final finding count is `0`
   - every remaining work item is assigned to a sprint
   - missing sprint docs were created if needed
   - branch was pushed and validation reported

`source_of_truth` should point at the already-approved planning sources:
- reviewed planning docs in the repo
- a verbatim user-approved plan capsule
- explicit references to prior planning discussion already completed with
  `arch-ctm`

If `questions_or_concerns` is present, `arch-ctm` should answer it in the ACK.

The ACK should also include a brief outline of the plan/work that `arch-ctm`
understands to be in scope. `team-lead` should wait for that ACK and outline
before raising scope concerns or discussing adjustments with the user.

Render:
- `.claude/skills/plan-hardening/plan-hardening.xml.j2`

Example:

```bash
sc-compose render \
  --root .claude/skills/plan-hardening \
  --file plan-hardening.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json
```

Suggested vars file shape:

```json
{
  "task_id": "TASK-1234",
  "phase": "phase-S",
  "description": "Harden the second half of Phase S before implementation resumes.",
  "worktree_path": "/abs/worktree",
  "branch": "feature/pS-plan-hardening",
  "pr_target": "integrate/phase-S",
  "source_of_truth": "- User-approved planning discussion already completed with arch-ctm\n- docs/project-plan.md\n- docs/plan-phase-S.md\n- docs/requirements.md\n- docs/architecture.md",
  "questions_or_concerns": "- Confirm whether missing follow-on sprints must be created on this branch if the current phase plan stops too early.",
  "references": "- docs/project-plan.md\n- docs/plan-phase-S.md\n- docs/requirements.md\n- docs/architecture.md"
}
```

## Guardrails

- do not send the task before the worktree exists
- do not rewrite the plan into a freeform summary
- do not let the task stop while remaining work lacks sprint ownership
- do not accept a phase plan that ends before the remaining work ends
