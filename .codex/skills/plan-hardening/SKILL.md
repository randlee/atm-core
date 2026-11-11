---
name: plan-hardening
version: 0.19.16
description: >
  Harden a phase plan in beads before implementation: scope review, critical
  review and ceremony review, each followed by a planner fix pass, then the
  plan gate. Use when a phase plan file exists and needs hardening, or when
  the user asks for /plan-hardening. Runs as a planner with subagents, a
  planner with teammates, or the phase lead adversarially to the planner.
---

# Plan Hardening

Use this only for phase-plan hardening before implementation starts or resumes.

## Step 1 — Verify CLI Installation

```bash
for c in bd atm sc-compose jq; do command -v "$c" >/dev/null && echo "ok $c" || echo "MISSING $c"; done
```

If anything is missing, **read
`.claude/skills/atm-beads/references/installation-and-troubleshooting.md`
before proceeding.**

## Modes

Pick one and read only its reference:

| Mode | Runs steps | Reference |
|------|------------|-----------|
| planner with subagents (team-mates busy) | the planner | `references/mode-planner-subagents.md` |
| planner with teammates | the planner | `references/mode-planner-teammates.md` |
| lead adversarial to the planner | the phase lead | `references/mode-adversarial-lead.md` |

"The runner" below is whoever runs the steps; "the lead" is the phase lead.

## Assumptions

- the plan is in beads: the plan file `docs/plans/phase-<phase>.jsonl` is
  committed, though sprint beads may still be partial or missing
- do not ask the user to explain detailed plan content; read the planning docs
  and references directly after they are created
- the runner routes the process; only the planner rewrites the plan
- the user-discussed deliverable scope is authoritative
- if no target phase worktree exists, create one on the phase's integration
  branch before starting
- the runner must not redirect hardening flow,
  offer accept-and-proceed shortcuts, or stop to ask the user what to do when
  an in-scope hardening loop is still mechanically actionable

## Expected Result

Sprint plan approved by:
- `plan-scope-reviewer`
- `critical-plan-reviewer`
- `ceremony-qa`
- `quality-mgr` (the plan gate)

The approved plan is a set of parallel tracks with non-intersecting
`owned_paths`, aiming at the shortest critical path with the fewest sprints.
A cross-boundary feature is cut into a contract sprint, parallel layer
sprints that each close one boundary against contract tests, and its own
integration sprint that owns its end-to-end criteria. Independent changes
and single-boundary features stay as their own sprints or stacked tracks;
thin layer sprints are as much a defect as full-stack feature sprints. The
phase plan carries a wave table with critical path, width and sprint count,
and every name follows "Naming" in the guidelines. Hardening exists to make that shape safe to run in
parallel; it must never make a plan more serial. Record `critical_path` and
`width` from the `plan-scope-reviewer` output in the Step 2 bead's close reason,
and treat a round that lengthens the critical path as a regression to
be explained.

## Required Reference

Always use:
- `.claude/skills/atm-beads/resources/atm-beads-plan-guidelines.md`

## Execution Table

| # | Route to | Input required | Output expected | Read before executing |
|---|----------|----------------|-----------------|-----------------------|
| 1 | planner | vars file | `step-1` fenced JSON | `steps/step-1.md` |
| 2 | `plan-scope-reviewer` | context + `step-1` JSON | `step-2` fenced JSON | `steps/step-2.md` |
| 3 | planner | `step-2` JSON | `step-3` fenced JSON | `steps/step-3.md` |
| 4 | `critical-plan-reviewer` | context + `step-3` JSON | `step-4` fenced JSON | `steps/step-4.md` |
| 5 | planner | `step-4` JSON | `step-5` fenced JSON | `steps/step-5.md` |
| 6 | `ceremony-qa` | `step-5` JSON | `step-6` fenced JSON | `steps/step-6.md` |
| 7 | `quality-mgr` | `step-6` JSON | the plan gate | `steps/step-7.md` |

## Beads

Every step run is a step bead and every reviewer finding is a finding bead,
both children of the phase root:

- before dispatching a step, the runner imports its step bead
  (`plan-hardening-step-bead.json.j2`, id `<root>-ph-s<step>-c<cycle>`,
  `blocked_by` the previous step bead and, for a review step, the finding
  beads of the previous review) and sets `task_id` in the vars to that id;
  dispatch it only while `bd ready -l phase-<phase>,stage:plan-hardening`
  lists it
- after a review step (2, 4, 6), the runner files one finding bead
  (`plan-hardening-finding-bead.json.j2`, id `<step bead>-<finding id in
  lower case>`) per finding in the reviewer's fenced JSON, then closes the
  step bead with `bd close <id> --reason "<status> blocking=<n> important=<n>
  minor=<n> reviewed_commit=<sha> findings_hash=<hash>"`
- the planner closes each open finding bead in the next correction step with
  its ruling; the runner closes the planner's step bead with its status
- `bd list -l phase-<phase>,stage:plan-hardening --status closed` is the round table;
  `bd list -l phase-<phase>,stage:plan-finding` is the open findings

Import a rendered bead with `jq -c . <file> > <file>.jsonl && bd import <file>.jsonl`.

## Reviewer Cycle Caps

- `plan-scope-reviewer` and `critical-plan-reviewer` both default to a
  2-cycle cap
- these caps must be carried in JSON:
  - `plan_scope_review_cycle_limit`
  - `critical_review_cycle_limit`
- reviewer launch payloads must also include:
  - `review_cycle_limit`
  - `review_cycle_index`
- if the vars JSON omits the cap fields, the runner must default them to `2`

Cycle-cap behavior:

- every `FAIL` from `plan-scope-reviewer` or `critical-plan-reviewer` must be
  routed to the planner immediately through the matching plan-editing step
- no reviewer findings may be accepted as-is or bypass the planner
- a `FAIL` from `critical-plan-reviewer` on its final cycle is normal: the
  review is adversarial. Its findings go to Step 5, which fixes them, and the
  run continues to the ceremony review
- if `plan-scope-reviewer` returns `FAIL` on its final allowed cycle, the
  runner must still send those findings to the planner for one final
  correction pass, then continue to Step 3
- do not ask the user how to proceed, do not offer multiple-choice options,
  and do not invent an "accept and proceed" path

## Hard Stops

- the runner only checks the top-level `status` and expected `mode` fields on
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
  production-ready level for its closure type, split it along boundaries into
  sibling sprints before implementation; a serial split of a feature sprint
  is not an accepted correction
- when a reviewer loop reaches its configured cap, route the last findings to
  the planner as the cycle-cap rules say; do not launch another review of
  that kind and do not ask the user for a decision mid-loop

## Workflow Metadata

Every template in this skill declares ATM template metadata
(`metadata.type`, `metadata.tags`, `metadata.workflow`; see
`docs/template-workflow-metadata.md`). The declared stage is `plan`, so any
message rendered from one of them is discoverable without reading its body:

```sh
atm search --team <team> --workflow-stage plan --since <ISO> --json
atm search --team <team> --type 'plan-*' --since <ISO> --json
```

Rounds that run a reviewer as a background agent (steps 2, 4 and 6) produce no
ATM message on their own; the step docs require a `plan-review-notice` send
after each such round so the run stays observable. Rounds routed to a team
agent instead of a background agent use `plan-critical-review.xml.j2` (or
the numbered assignment templates) and are recorded by that dispatch.
Install the templates before the first send:

```sh
mkdir -p ~/.atm/templates/plan-hardening && cp .claude/skills/plan-hardening/*.j2 ~/.atm/templates/plan-hardening/
```

## Render

- `.claude/skills/plan-hardening/01-plan-scope-review.xml.j2`
- `.claude/skills/plan-hardening/02-sprint-scope-hardening.xml.j2`
- `.claude/skills/plan-hardening/03-consistency-hardening.xml.j2`
- `.claude/skills/plan-hardening/plan-critical-review.xml.j2`
- `.claude/skills/plan-hardening/plan-review-notice.xml.j2`

Each step's task closes with its complete template
(`atm task close <task-id> completed --template <file> --vars <file>`):
- `.claude/skills/plan-hardening/plan-scope-review-complete.md.j2`
- `.claude/skills/plan-hardening/plan-sprint-hardening-complete.md.j2`
- `.claude/skills/plan-hardening/plan-consistency-hardening-complete.md.j2`
- `.claude/skills/plan-hardening/plan-critical-review-complete.md.j2`

Beads are rendered from
`.claude/skills/plan-hardening/plan-hardening-step-bead.json.j2` and
`.claude/skills/plan-hardening/plan-hardening-finding-bead.json.j2`.

Start the vars file from
`.claude/skills/plan-hardening/examples/plan-hardening-vars.example.json`.
