---
name: plan-hardening
version: 1.2.0
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

The user-discussed deliverable scope is authoritative. Hardening should welcome
improvements that clarify, tighten, split, or otherwise make the plan more
executable when those improvements are consistent with what the user already
discussed with `arch-ctm`. Hardening must resist and push back on substantial
scope changes that are at odds with that user discussion.

## Preconditions

- the target phase worktree already exists
- `worktree_path` and `branch` are known
- `sc-compose` is available
- the plan has already been discussed with `arch-ctm`

## Required Reference

Always use:
- `.claude/skills/plan-hardening/sprint-planning-guidelines.md`

## Expected Result

The task must end with this required sequence:
- step 1: `plan-develop`
- step 2: `plan-scope-reviewer`
- step 3: sprint-scope hardening
- step 4: `critical-plan-reviewer`
- step 5: document-consistency hardening
- step 6: focused plan QA

Together they must produce:
- complete/consistent planning docs
- a hardened sprint doc for every sprint still required to finish the phase
- no unassigned in-scope implementation work
- no overloaded sprint whose deliverables cannot all land at a production-ready level
- explicit code samples or signatures for important traits, features, enums,
  protocol types, and boundary contracts
- branch pushed, validation reported, team-lead critical review completed or
  explicitly requested, and QA requested only after both passes and that review

Substantial scope changes are not valid hardening output. Examples:
- dropping, replacing, or weakening a user-discussed deliverable
- converting a runtime/code sprint into a docs/lint-only sprint
- retargeting work to a different phase or integration branch
- adding a new deliverable that materially changes the implementation outcome
  promised to the user

Any remaining in-scope work without sprint ownership is a `GAP`. If more
sprints are needed, hardening must create them.

## Team-Lead Steps

1. Prepare:
   - `phase_id`
   - `task_id`
   - `description`
   - `worktree_path`
   - `branch`
   - `sprint_doc`
   - `pr_target`
   - `source_of_truth`
   - `rough_plan_json`
   - optional `questions_or_concerns`
   - `references`
2. Render `.claude/skills/plan-hardening/plan-develop.xml.j2` with
   `sc-compose` for formal plan creation from the rough plan.
3. Send the rendered `plan-develop.xml.j2` ATM task to `plan-develop`.
4. Review the `plan-develop` result:
   - the result includes fenced JSON
   - `ready_for_next_step` is `true`, or the planner reported a concrete blocker
   - the docs are ready for sprint-scope review
5. Render `.claude/skills/plan-hardening/plan-scope-review.xml.j2` with
   `sc-compose` for the sprint-scope review pre-pass. Pass the fenced JSON
   output from `plan-develop` as required input.
6. Send the rendered `plan-scope-review.xml.j2` ATM task to
   `plan-scope-reviewer`.
7. Review the sprint-scope review result:
   - final finding count is `0`, or findings are routed into the next
     hardening pass for correction
   - split risk, drop risk, and checklist-shape risk are explicit
   - the result includes fenced JSON
8. Render `.claude/skills/plan-hardening/plan-hardening.xml.j2` with
   `sc-compose` for sprint-scope hardening. Pass the fenced JSON output from
   `plan-scope-reviewer` as required input.
9. Send the rendered sprint-scope hardening ATM task to `arch-ctm`.
10. Review the sprint-scope hardening result:
   - final finding count is `0`
   - every remaining work item is assigned to a sprint
   - missing sprint docs were created if needed
   - every committed deliverable is assigned to exactly one sprint
   - if any sprint was overloaded or had production-ready risk, it was split
   - important traits/features/enums/boundaries have explicit code samples
   - branch was pushed and validation reported
   - the result includes fenced JSON
11. Render `.claude/skills/plan-hardening/critical-plan-review.xml.j2` with
   `sc-compose` for the late hostile plan review. Pass the fenced JSON output
   from sprint-scope hardening as required input.
12. Send the rendered critical-plan-review ATM task to
   `critical-plan-reviewer`.
13. Review the critical-plan-review result:
   - final finding count is `0`, or findings are routed into the next
     hardening pass for correction
   - architecture risk, boundary risk, and false-closure risk are explicit
   - the result includes fenced JSON
14. Render `.claude/skills/plan-hardening/plan-hardening-consistency.xml.j2`
   with `sc-compose` for document-consistency hardening. Pass the fenced JSON
   output from `critical-plan-reviewer` as required input.
15. Send the rendered consistency hardening ATM task to `arch-ctm`.
16. Review the consistency hardening result:
   - final finding count is `0`
   - cross-document contradictions are resolved
   - boundary ownership and ADR coverage are explicit
   - ambiguous wording is removed
   - branch was pushed and validation reported
   - the result includes fenced JSON
17. Critically review the hardened plan before QA:
   - read the updated phase plan and every new or changed sprint doc
   - review sprint deliverables for concrete ownership, production-ready scope,
     and execution clarity
   - review acceptance criteria for explicit, testable closeout gates
   - review whether any sprint still appears overloaded and should be split
   - review whether any important trait/feature/enum is still promised without
     an explicit code sample or signature
   - push back on vague wording, missing deliverables, or unverifiable
     acceptance criteria
   - request another hardening pass from `arch-ctm` if the plan is still
     ambiguous
18. Only after the critical review passes, route the plan to `quality-mgr` for a
   focused plan QA review.

`source_of_truth` should point at the already-approved planning sources:
- reviewed planning docs in the repo
- a verbatim user-approved plan capsule
- explicit references to prior planning discussion already completed with
  `arch-ctm`

If `source_of_truth`, `questions_or_concerns`, or repo documents imply a
substantial scope change from what the user already discussed with `arch-ctm`,
that is a stop condition. The hardening pass must not rewrite the sprint scope
to match it. Push back to `team-lead`, describe the scope conflict explicitly,
and require user discussion before proceeding.

If `questions_or_concerns` is present, `arch-ctm` should answer it in the ACK.

The ACK should also include a brief outline of the plan/work that `arch-ctm`
understands to be in scope. `team-lead` should wait for that ACK and outline
before raising scope concerns or discussing adjustments with the user.

After both hardening passes complete, `team-lead` should do a second,
critical review focused on whether:
- sprint deliverables are split across sprints adequately
- every committed deliverable is expected to land at a production-ready level
- any sprint still has too many deliverables and should be split now
- sprint deliverables are concrete enough that a dev agent can prove presence
- acceptance criteria are explicit enough that `req-qa` can verify them
- important traits/features/enums/boundaries have explicit code samples or
  signatures
- any remaining residual work still lacks sprint ownership
- any hardening change materially altered the user-discussed deliverable scope
  without explicit user discussion

Do not treat the hardening pass itself as the final review. The handoff is:
1. `team-lead` routes `plan-develop`
2. `team-lead` routes `plan-scope-reviewer`
3. `arch-ctm` completes sprint-scope hardening to zero findings
4. `team-lead` routes `critical-plan-reviewer`
5. `arch-ctm` completes consistency hardening to zero findings
6. `team-lead` critically reviews and pushes back if needed
7. `quality-mgr` performs focused plan QA after that review

Every step after `plan-develop` must receive the previous step's fenced JSON
output as a required input artifact. Missing or malformed prior-step JSON is a
hard stop.

Render:
- `.claude/skills/plan-hardening/plan-develop.xml.j2`
- `.claude/skills/plan-hardening/plan-scope-review.xml.j2`
- `.claude/skills/plan-hardening/plan-hardening.xml.j2`
- `.claude/skills/plan-hardening/critical-plan-review.xml.j2`
- `.claude/skills/plan-hardening/plan-hardening-consistency.xml.j2`
- `.claude/skills/plan-hardening/sprint-planning-guidelines.md`

Example:

```bash
sc-compose render \
  --root .claude/skills/plan-hardening \
  --file plan-develop.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json

sc-compose render \
  --root .claude/skills/plan-hardening \
  --file plan-scope-review.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json

sc-compose render \
  --root .claude/skills/plan-hardening \
  --file plan-hardening.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json

sc-compose render \
  --root .claude/skills/plan-hardening \
  --file critical-plan-review.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json

sc-compose render \
  --root .claude/skills/plan-hardening \
  --file plan-hardening-consistency.xml.j2 \
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
  "sprint_doc": "docs/phase-S/sprint-S1.md",
  "pr_target": "integrate/phase-S",
  "rough_plan_json": "```json\n{\n  \"status\": \"PASS\",\n  \"docs_created\": [],\n  \"docs_modified\": [\"docs/plan-phase-S.md\"],\n  \"sprints_in_scope\": [\"S.1\", \"S.2\"],\n  \"ready_for_next_step\": true,\n  \"notes\": [],\n  \"errors\": []\n}\n```",
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
- do not accept a sprint that carries more deliverables than can credibly land
  at a production-ready level
- do not allow a committed deliverable to become optional, implicit, or silently
  deferred
- do not allow important traits/features/enums/boundary contracts to stay
  prose-only when an explicit code sample or signature is needed
- do welcome improvements that make the plan more explicit, production-ready,
  or better split as long as they stay consistent with the user-discussed scope
- do not let hardening silently rewrite the deliverable scope discussed with
  the user
- if `team-lead` input conflicts materially with that scope, stop and push
  back instead of normalizing the new scope into the docs
- do not send the hardened plan to QA before `team-lead` has critically
  reviewed deliverables, sprint splitting, code-sample completeness, and
  acceptance criteria
