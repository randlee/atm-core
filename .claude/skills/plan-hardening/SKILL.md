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
been discussed and are fresh in arch-ctm context. The current plan state
already exists in repo docs, though sprint docs may still be partial or
missing. Do not expect `team-lead` to explain detailed plan content; read the
planning docs and references directly.

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
- `.claude/skills/plan-hardening/plan-hardening-state-machine.md`

Execution standard:
- the `/plan-hardening` flow must be simple enough for a low-context `team-lead` to execute by following the numbered templates in order
- `team-lead` should not need to understand, restate, or reinterpret the plan details
- if a step requires `team-lead` to improvise or explain the plan, the docs or prompt chain are not hardened enough

## Expected Result

The task must end with this required sequence:
- step 1: initial guidelines pass by `arch-ctm`
- step 2: background `plan-scope-reviewer`
- step 3: sprint-scope hardening by `arch-ctm`
- step 4: background `critical-plan-reviewer`
- step 5: document-consistency hardening by `arch-ctm`
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

Routing summary:
- `team-lead -> arch-ctm` with `01-plan-scope-review.xml.j2`
- `team-lead -> plan-scope-reviewer` with `02-plan-scope-review.xml.j2` and `run_in_background: true`
- `team-lead -> arch-ctm` with `03-sprint-scope-hardening.xml.j2`
- `team-lead -> critical-plan-reviewer` with `04-critical-plan-review.xml.j2` and `run_in_background: true`
- `team-lead -> arch-ctm` with `05-consistency-hardening.xml.j2`
- `team-lead -> quality-mgr` for final focused plan QA

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
2. Render `.claude/skills/plan-hardening/01-plan-scope-review.xml.j2` with
   `sc-compose`.
3. Send the rendered `01-plan-scope-review.xml.j2` ATM task to `arch-ctm`.
4. Wait for the fenced JSON output from step 1. Do not launch the reviewer
   until step 1 JSON exists and is well formed.
5. Render `.claude/skills/plan-hardening/02-plan-scope-review.xml.j2` with
   `sc-compose`. Pass the fenced JSON output from step 1 as required input.
6. Launch `plan-scope-reviewer` with the rendered
   `02-plan-scope-review.xml.j2` in background mode.
7. Wait for the fenced JSON findings from `plan-scope-reviewer`.
8. Review the scope-review result:
   - final finding count is `0`, or findings are routed into the next
     hardening pass for correction
   - split risk, drop risk, and checklist-shape risk are explicit
   - the result includes fenced JSON
9. Render `.claude/skills/plan-hardening/03-sprint-scope-hardening.xml.j2`
   with `sc-compose` for sprint-scope hardening. Pass the fenced JSON output
   from step 2 as required input.
10. Send the rendered sprint-scope hardening ATM task to `arch-ctm`.
11. Review the sprint-scope hardening result:
   - final finding count is `0`
   - every remaining work item is assigned to a sprint
   - missing sprint docs were created if needed
   - every committed deliverable is assigned to exactly one sprint
   - if any sprint was overloaded or had production-ready risk, it was split
   - important traits/features/enums/boundaries have explicit code samples
   - branch was pushed and validation reported
   - the result includes fenced JSON
12. Render `.claude/skills/plan-hardening/04-critical-plan-review.xml.j2`
   with `sc-compose` for the late hostile plan review. Pass the fenced JSON
   output from step 3 as required input.
13. Launch `critical-plan-reviewer` with the rendered
   `04-critical-plan-review.xml.j2` in background mode.
14. Wait for the fenced JSON findings from `critical-plan-reviewer`.
15. Review the critical-plan-review result:
   - final finding count is `0`, or findings are routed into the next
     hardening pass for correction
   - architecture risk, boundary risk, and false-closure risk are explicit
   - the result includes fenced JSON
16. Render `.claude/skills/plan-hardening/05-consistency-hardening.xml.j2`
   with `sc-compose` for document-consistency hardening. Pass the fenced JSON
   output from step 4 as required input.
17. Send the rendered consistency hardening ATM task to `arch-ctm`.
18. Review the consistency hardening result:
   - final finding count is `0`
   - cross-document contradictions are resolved
   - boundary ownership and ADR coverage are explicit
   - ambiguous wording is removed
   - branch was pushed and validation reported
   - the result includes fenced JSON
19. Critically review the hardened plan before QA:
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
20. Only after the critical review passes, route the plan to `quality-mgr` for a
   focused plan QA review.

`source_of_truth` and `references` should point at the current plan state and
already-approved planning sources:
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
1. `team-lead` routes `01-plan-scope-review.xml.j2` to `arch-ctm`
2. `arch-ctm` returns fenced JSON from the guidelines pass
3. `team-lead` launches `plan-scope-reviewer` in background mode with
   `02-plan-scope-review.xml.j2`
4. `plan-scope-reviewer` returns fenced JSON findings
5. `team-lead` routes `03-sprint-scope-hardening.xml.j2` to `arch-ctm`
6. `arch-ctm` returns fenced JSON from sprint-scope hardening
7. `team-lead` launches `critical-plan-reviewer` in background mode with
   `04-critical-plan-review.xml.j2`
8. `critical-plan-reviewer` returns fenced JSON findings
9. `team-lead` routes `05-consistency-hardening.xml.j2` to `arch-ctm`
10. `arch-ctm` returns fenced JSON from consistency hardening
11. `team-lead` critically reviews and pushes back if needed
12. `quality-mgr` performs focused plan QA after that review

Every step after step 1 must receive the previous step's fenced JSON output as
a required input artifact. Missing or malformed prior-step JSON is a hard
stop.

Render:
- `.claude/skills/plan-hardening/01-plan-scope-review.xml.j2`
- `.claude/skills/plan-hardening/02-plan-scope-review.xml.j2`
- `.claude/skills/plan-hardening/03-sprint-scope-hardening.xml.j2`
- `.claude/skills/plan-hardening/04-critical-plan-review.xml.j2`
- `.claude/skills/plan-hardening/05-consistency-hardening.xml.j2`
- `.claude/skills/plan-hardening/sprint-planning-guidelines.md`
- `.claude/skills/plan-hardening/plan-hardening-state-machine.md`

Example:

```bash
sc-compose render \
  --root .claude/skills/plan-hardening \
  --file 01-plan-scope-review.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json

sc-compose render \
  --root .claude/skills/plan-hardening \
  --file 02-plan-scope-review.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json

sc-compose render \
  --root .claude/skills/plan-hardening \
  --file 03-sprint-scope-hardening.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json

sc-compose render \
  --root .claude/skills/plan-hardening \
  --file 04-critical-plan-review.xml.j2 \
  --var-file /tmp/plan-hardening-vars.json

sc-compose render \
  --root .claude/skills/plan-hardening \
  --file 05-consistency-hardening.xml.j2 \
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
- do not accept a sprint that carries more deliverables than can credibly land
  at a production-ready level
- do not allow a committed deliverable to become optional, implicit, or silently
  deferred
- do not allow important traits/features/enums/boundary contracts to stay
  prose-only when an explicit code sample or signature is needed
- do not require `team-lead` to understand or restate detailed plan content;
  that detail must come from the plan docs and references
- do welcome improvements that make the plan more explicit, production-ready,
  or better split as long as they stay consistent with the user-discussed scope
- do not let hardening silently rewrite the deliverable scope discussed with
  the user
- if `team-lead` input conflicts materially with that scope, stop and push
  back instead of normalizing the new scope into the docs
- do not send the hardened plan to QA before `team-lead` has critically
  reviewed deliverables, sprint splitting, code-sample completeness, and
  acceptance criteria
