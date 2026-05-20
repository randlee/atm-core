---
id: PHI-PLAN-HARDENING-LOOP-CLEANUP-1
status: complete
branch: plan/plan-hardening-improvements
worktree: ../atm-core-worktrees/plan/plan-hardening-improvements
created: 2026-05-20
---

# PHI-PLAN-HARDENING-LOOP-CLEANUP-1: Plan-Hardening Loop Cleanup

## Motivation

Phase Yd plan-hardening ran 5 critical-plan-reviewer rounds before QA handoff, with two recurring failure modes:
1. **Stale replay**: arch-ctm pattern-matched new step-3 task messages as historical backlog and dismissed them (happened once per session).
2. **Incremental churn**: minor wording findings mixed with structural blockers, inflating round counts. Round 4 introduced new minor findings (net +1), round 5 converged slowly.

## Scope

Improve `.claude/skills/plan-hardening/` templates, step docs, and vars example so that:
- Stale replays are prevented by construction (not by workaround)
- Reviewer passes return all remaining B+I findings in one sweep
- Minor wording findings are separated from structural blockers
- Team-lead has a round-tracking table to detect non-convergence early

## Deliverables

### D1 — Round-tracking fields in vars template

Add to `examples/plan-hardening-vars.example.json`:
```json
"round_id": "STEP3A",
"round_index": 1,
"reviewer": "critical-plan-reviewer",
"reviewed_commit": "<sha>",
"previous_reviewed_commit": "",
"findings_hash": "",
"supersedes_task_id": ""
```

Add to `02-sprint-scope-hardening.xml.j2` template body (inside `<atm-task>`):
```xml
<round id="{{ round_id }}" index="{{ round_index }}" reviewed-commit="{{ reviewed_commit }}" supersedes="{{ supersedes_task_id }}"/>
```

### D2 — Stale replay detection in step-3

Update `.claude/skills/plan-hardening/steps/step-3.md`:
- Add rule: a new step-3 task is valid only when `reviewed_commit` differs from `previous_reviewed_commit` OR `findings_hash` of the new step-4 JSON differs from the prior round's findings_hash.
- Add rule: if arch-ctm ACKs but returns "already closed" or similar without a new fenced JSON, team-lead must re-send with an updated `round_id` (increment suffix) and an explicit `<replay-nonce>` field containing the current UTC timestamp.
- Add `<replay-nonce>{{ replay_nonce }}</replay-nonce>` to `02-sprint-scope-hardening.xml.j2` so every render produces a structurally unique message.

### D3 — Reviewer prompt tightening in step-4

Update `.claude/skills/plan-hardening/steps/step-4.md`:
- Add rule: reviewer must return **all** open Blocking + Important findings in a single pass. Returning a subset (finding new issues after prior ones close) is only permitted if the sprint doc changed between rounds.
- Add rule: Minor findings that do not affect implementability (pure wording, formatting, non-normative prose) must be collected into a separate `minor_wording` array in the fenced JSON and do NOT contribute to FAIL status unless the reviewer flags them as `affects_ac: true`.
- Update the fenced JSON schema in step-4 to include `minor_wording: []` array and `affects_ac` per-finding flag.

### D4 — Team-lead convergence table in step-3

Update `.claude/skills/plan-hardening/steps/step-3.md`:
- Add a "round convergence tracking" section with a template table:
  ```
  | Round | reviewed_commit | B | I | m | Total | Δ |
  ```
- Add rule: if round N has Total >= round N-2 (non-convergence), team-lead must escalate to user before dispatching round N+1.

### D5 — Separate structural from wording in sprint-planning-guidelines

Update `.claude/skills/plan-hardening/sprint-planning-guidelines.md`:
- Add section "Finding classification":
  - Structural: missing AC gate, incorrect rg command, uncovered call site, missing function, false-closure — always Blocking or Important, always FAIL
  - Wording: prose ambiguity, optional formatting, non-normative text — Minor, does NOT FAIL unless affects_ac: true
- Add rule: reviewers must classify each finding as structural or wording before assigning severity.

## Acceptance Criteria

- `examples/plan-hardening-vars.example.json` includes all 7 new round-tracking fields
- `02-sprint-scope-hardening.xml.j2` includes `<round .../>` and `<replay-nonce>` elements
- `steps/step-3.md` includes stale replay detection rule and convergence tracking table template
- `steps/step-4.md` includes "return all B+I in one pass" rule and `minor_wording` array schema
- `sprint-planning-guidelines.md` includes finding classification section
- `git diff --check` clean
- No regressions to existing template rendering (verify with `python3 -c "from jinja2 import Template; ..."` spot check)

## Required Validation

```bash
# Verify all target files modified
git -C /Users/randlee/Documents/github/atm-core-worktrees/plan/plan-hardening-improvements diff --name-only HEAD

# Verify no whitespace errors
git -C /Users/randlee/Documents/github/atm-core-worktrees/plan/plan-hardening-improvements diff --check HEAD

# Spot-check template renders without error
python3 -c "
from jinja2 import Template
import json
with open('.claude/skills/plan-hardening/02-sprint-scope-hardening.xml.j2') as f:
    t = f.read().split('---\n', 2)[-1]
Template(t).render(task_id='TEST', phase='test', description='test', worktree_path='/tmp', branch='test', pr_target='develop', source_of_truth='', references='', previous_step_json='{}', round_id='STEP3A', round_index=1, reviewed_commit='abc', supersedes_task_id='')
print('Template render OK')
"
```
