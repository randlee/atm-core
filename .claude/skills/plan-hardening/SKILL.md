---
name: plan-hardening
version: 1.0.0
description: >
  Orchestrate a full documentation hardening pass for a multi-sprint development
  phase before implementation starts or resumes. Use when team-lead needs
  arch-ctm to capture all known plan decisions, create any missing sprint docs,
  and loop on audit/fix until the phase docs are complete, consistent, and
  execution-ready.
depends_on:
  codex-orchestration: 0.x
---

# Plan Hardening

Audience: `team-lead` only.

Use this skill when a phase plan exists but the phase is not ready to execute
because the documentation set may still be incomplete, inconsistent, or missing
follow-on sprint decomposition.

This skill is specifically for the "before implementation starts" or
"before implementation resumes" hardening pass that turns a known plan into an
execution-ready document set.

## When To Use It

Use `/plan-hardening` when one or more of these are true:

- the phase has known product or architecture decisions that are not yet landed
  in the required docs
- requirements or ADRs mention remaining work that is not yet assigned to a
  specific sprint
- the phase currently stops at `S.N`, but everyone already knows more sprints
  will be needed to finish the phase
- protocol/schema/boundary docs changed, but the sprint sequence has not been
  reconciled to those changes
- the team wants a "zero findings" hardening loop before starting or resuming a
  multi-sprint execution line

Do not use this skill for ordinary implementation or QA fix rounds. Use it only
for the planning/documentation hardening step that `team-lead` delegates to
`arch-ctm`.

Use it only after the user discussion is complete enough that `team-lead`
understands what the hardening task must accomplish and can delegate one stable
task scope to `arch-ctm`.

## Prerequisites

Before dispatching a hardening task:

1. The target phase worktree exists and is on the correct branch.
2. The user or team already knows the substantive plan decisions.
3. `team-lead` can provide a concise `plan_context` summary in the assignment.
4. `sc-compose` is available for rendering the XML assignment template.

If the plan is still being invented, finish the design discussion first. This
skill assumes the plan is already known and needs to be captured and hardened.

## Required Outputs

The hardening task must leave the phase with:

- a top-level project plan that reflects the phase status
- a phase plan document that covers the whole remaining phase
- a hardened sprint doc for every sprint required to finish the phase
- product requirements and architecture aligned with the phase
- crate-local requirements/architecture docs for every affected crate
- ADRs for every significant architectural decision plus an updated ADR index
- protocol/schema/interface docs for every touched boundary
- machine-readable boundary definitions for every boundary in scope
- an issues inventory that matches the current state
- testing and cross-platform guidance when the phase touches those concerns

Critical rule:
- any remaining in-scope implementation work not assigned to a specific sprint
  doc is a `GAP` finding
- if more sprints are needed beyond the currently documented set, new sprint
  docs must be created during hardening

## Team-Lead Workflow

### 1. Assemble the plan context

Prepare:

- `phase_id`
- `task_id`
- `description`
- `worktree_path`
- `branch`
- `pr_target`
- `plan_context`
- `references`

`plan_context` must summarize the known plan decisions that are already agreed
but may not yet be fully documented. Keep it concise but concrete. The
assignment tells `arch-ctm` not to rediscover the plan from scratch.

### 2. Render the assignment template for `arch-ctm`

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
  "plan_context": "- Remaining work requires S.6-S.8 ...\n- atm list / atm read split is already decided ...",
  "references": "- docs/project-plan.md\n- docs/plan-phase-S.md\n- docs/requirements.md\n- docs/architecture.md"
}
```

### 3. Send the rendered XML to `arch-ctm`

The rendered template is an ATM task message whose assignee is `arch-ctm`.
`team-lead` uses this skill to delegate the hardening work; `team-lead` does
not perform the hardening loop directly.

### 4. Review the hardening result

Before accepting the result:

- confirm the hardening report says `Final finding count: 0`
- confirm every remaining in-scope work item is assigned to a sprint
- confirm missing sprint docs were created if the phase needed more sprints
- confirm the branch is pushed and validation is reported

### 5. Route plan QA

After the hardening push and validation report:

- request a `quality-mgr` plan review if the phase is important enough to gate
  before implementation

## Template Contract

The assignment template bakes in these hard requirements:

- the plan is already known and must be captured before hardening begins
- all required document classes are audited
- missing sprint docs are `GAP` findings
- all unassigned remaining implementation work is `GAP`
- sprint docs must be execution-ready, not just placeholders
- audit/fix repeats until zero findings
- the hardening work is delegated to `arch-ctm` through ATM, not executed
  inline by `team-lead`

## Anti-Patterns

- Do not send a hardening task without `plan_context`.
- Do not allow the agent to stop after "requirements and architecture are good"
  while remaining work still lacks sprint ownership.
- Do not accept a phase plan that ends before the remaining work ends.
- Do not treat ADR or architecture text as sufficient if no sprint owns the
  implementation.
