# Phase Post-Mortem

Use this reference at the end of a phase after:
- all sprint work is complete
- all sprint branches have been integrated into `integrate/phase-X`
- all queued QA findings are resolved or intentionally deferred
- follow-up QA has closed the phase findings set
- a final phase-ending review is run on `integrate/phase-X` before the merge to
  `develop`

Audience:
- `team-lead`
- `arch-ctm`
- `quality-mgr`

Goal:
- learn from the full phase finding set
- identify systemic fixes, not just branch-local code fixes

## Inputs

Review:
- all `.triage/<phase_id>/findings/*.ttl` records
- final QA reports for the phase
- fix-dispatch batches sent to `arch-ctm`
- follow-up QA closeout messages from `quality-mgr`
- the final `integrate/phase-X` verification report from `quality-mgr`

## Required Integration Review

Before the final merge to `develop`, `quality-mgr` must run one full review on
`integrate/phase-X` after all sprint integrations are complete.

`quality-mgr` should deploy a background review team based on role, not rely on
one subagent. The exact mix depends on the phase scope, but it should cover the
same reviewer roles used during QA for the affected artifact types.

That review must verify:
- 100% of triaged findings for the phase are fixed or intentionally deferred
- no finding marked fixed on a sprint branch is still reproducible on
  `integrate/phase-X`
- merge-forward gaps did not re-open previously closed findings during
  integration
- no phase-scope fix was missed because it landed outside the original changed
  files or outside one reviewer role's scope

This is a required phase-ending quality gate, not an optional spot check.

## Required Review Questions

For each finding family or repeated failure pattern:
1. Was this a one-off bug, or a recurring class of defect?
2. Did the defect come from:
   - unclear requirements
   - architectural drift
   - missing boundary enforcement
   - missing lint or static verification
   - weak sprint planning
   - weak QA scoping
   - merge-forward failure
   - test coverage gap
3. Could the issue have been prevented earlier than QA?
4. What is the smallest durable prevention mechanism?

## Required Classifications

Every reviewed finding family should end in one or more of these outcomes:
- `new_adr`
- `new_lint`
- `boundary_update`
- `requirements_update`
- `architecture_update`
- `project_plan_update`
- `sprint_plan_update`
- `qa_process_improvement`
- `planning_process_improvement`
- `test_hardening`
- `merge_forward_process_improvement`
- `no_systemic_followup`

Also record one phase-level outcome:
- `integration_review_passed`
- `integration_review_failed`

## Expected Recommendations

### New ADR

Choose this when QA repeatedly exposes an unresolved design rule or ownership
boundary that should become explicit architecture policy.

### New lint

Choose this when the defect pattern is mechanically detectable and should be
blocked before QA.

Examples:
- forbidden construct
- missing gate
- naming rule
- boundary violation
- unsafe platform-specific import

### Planning process improvement

Choose this when the bug existed because the sprint plan, phase plan, or task
assignment omitted scope, sequencing, or acceptance criteria.

### QA process improvement

Choose this when QA or triage discovered the issue late because:
- reviewer scope was too narrow
- repeatable-pattern sweep was missing
- carry-forward context was missing
- round-limit rules were unclear

## Output Shape

Produce a concise post-mortem summary grouped by finding family:
- finding family / ids
- repeated pattern description
- root cause classification
- recommended systemic action
- owner
- target artifact

Target artifacts should be concrete:
- `docs/adr/...`
- `.claude/skills/...`
- `.claude/agents/...`
- `boundaries/...`
- lint script or template path
- planning document path

## Decision Rule

Prefer the smallest upstream control that prevents recurrence.

Order of preference:
1. lint / static enforcement
2. boundary enforcement
3. ADR / architecture clarification
4. planning or QA workflow improvement
5. repeated manual reviewer vigilance

If a repeatable pattern still relies mainly on manual QA attention, treat that
as a process gap unless automation is genuinely not practical.
