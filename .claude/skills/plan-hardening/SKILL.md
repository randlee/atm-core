---
name: plan-hardening
version: 1.4.0
description: >
  Team-lead drives plan hardening after the current plan state already exists
  in repo docs.
depends_on:
  codex-orchestration: 0.x
---

# Plan Hardening

Audience: `team-lead` only.

Use this only for phase-plan hardening before implementation starts or resumes.

## Assumptions

- the current plan state already exists in repo docs, though sprint docs may
  still be partial or missing
- do not ask the user to explain detailed plan content; read the planning docs
  and references directly after they are created
- `team-lead` routes the process but is not the authority for rewriting the
  plan
- the user-discussed deliverable scope is authoritative
- if no target phase worktree exists, create one from `develop` before
  starting

## Expected Result

Sprint plan approved by:
- `plan-scope-reviewer`
- `critical-plan-reviewer`
- `quality-mgr`

## Required Reference

Always use:
- `.claude/skills/plan-hardening/sprint-planning-guidelines.md`

## Execution Table

| # | Route to | Input required | Output expected | Read before executing |
|---|----------|----------------|-----------------|-----------------------|
| 1 | `arch-ctm` | vars file | `step-1` fenced JSON | `steps/step-1.md` |
| 2 | `plan-scope-reviewer` (background) | context + `step-1` JSON | `step-2` fenced JSON | `steps/step-2.md` |
| 3 | `arch-ctm` | `step-2` JSON | `step-3` fenced JSON | `steps/step-3.md` |
| 4 | `critical-plan-reviewer` (background) | context + `step-3` JSON | `step-4` fenced JSON | `steps/step-4.md` |
| 5 | `arch-ctm` | `step-4` JSON | `step-5` fenced JSON | `steps/step-5.md` |
| 6 | `quality-mgr` | `step-5` JSON + QA vars file | codex-orchestration plan-QA handoff | `steps/step-6.md` |

## Round Tracking

`team-lead` must keep a round table for every `/plan-hardening` run.

Minimum columns:

| Round | Step | Reviewer | reviewed_commit | status | blocking | important | minor | findings_hash | supersedes | Note |
|-------|------|----------|-----------------|--------|----------|-----------|-------|---------------|------------|------|

Use the example in:
- `.claude/skills/plan-hardening/examples/plan-hardening-rounds.example.md`

## Hard Stops

- `team-lead` only checks the top-level `status` and expected `mode` fields on
  each fenced JSON response before advancing
- every step after step 1 must receive the previous step's fenced JSON
- missing or malformed fenced JSON is a hard stop
- a reviewer rerun is valid only when either `reviewed_commit` changed or
  `findings_hash` changed
- if the same reviewer returns the same `reviewed_commit` and the same
  `findings_hash` again, treat it as a stale replay and do not open a new
  hardening round
- substantial scope drift from the user-discussed plan is a hard stop
- remaining in-scope work without sprint ownership is a hard stop
- if a sprint cannot credibly land its committed deliverables at a
  production-ready level, split it before implementation
- if a reviewer loop returns `FAIL` three times without converging, escalate to
  the user before continuing

## Render

- `.claude/skills/plan-hardening/01-plan-scope-review.xml.j2`
- `.claude/skills/plan-hardening/02-sprint-scope-hardening.xml.j2`
- `.claude/skills/plan-hardening/03-consistency-hardening.xml.j2`
- `.claude/skills/plan-hardening/steps/step-1.md`
- `.claude/skills/plan-hardening/steps/step-2.md`
- `.claude/skills/plan-hardening/steps/step-3.md`
- `.claude/skills/plan-hardening/steps/step-4.md`
- `.claude/skills/plan-hardening/steps/step-5.md`
- `.claude/skills/plan-hardening/steps/step-6.md`
- `.claude/skills/plan-hardening/examples/plan-hardening-vars.example.json`
- `.claude/skills/plan-hardening/examples/plan-hardening-rounds.example.md`
- `.claude/skills/plan-hardening/examples/plan-hardening-qa-vars.example.json`
- `.claude/skills/plan-hardening/sprint-planning-guidelines.md`
