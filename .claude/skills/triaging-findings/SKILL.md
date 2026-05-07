---
name: triaging-findings
version: 1.0.0
description: Orchestrate pre-dispatch QA finding triage as team-lead. Launch one qa-triage agent per finding, collect phase-scoped Turtle records, aggregate by promoted branch, and only then dispatch branch-scoped fix assignments to arch-ctm.
depends_on:
  codex-orchestration: 0.x
  quality-management-gh: 1.x
---

# Triaging Findings

Audience: `team-lead` only.

Use this skill when QA has produced findings and you need to correlate them
across worktrees before any fix work is sent to `arch-ctm`.

For phase-end learning and process hardening, also read:
- `references/post-mortem.md`

## Preconditions

Before using this workflow:
1. `.claude/agents/qa-triage.md` exists and is the active triage-agent prompt.
2. The target phase has an explicit `phase_id` such as `phase-R`.
3. The ordered worktree list is known in promotion order.
4. QA findings exist in a structured form with stable finding ids.
5. `sc-compose` is installed for rendering assignment templates.

## Ownership Model

- `quality-mgr` identifies and reports QA findings.
- `team-lead` owns triage orchestration and dev dispatch.
- `qa-triage` correlates evidence and writes canonical `.ttl` records.
- `arch-ctm` receives only post-triage, branch-scoped fix assignments.

Do not send raw QA findings directly to `arch-ctm`.

## Required Inputs

For each triage batch, assemble:
- `phase_id`
- `triage_root`
- ordered `worktrees` with branch, absolute path, head SHA, and order index
- finding records with:
  - `finding_id`
  - `title`
  - `description`
  - `severity`
  - `pattern`
  - `repeatable`
  - `sweep_scope`
- triage mode:
  - `initial_pass`
  - `followup_pass`

Canonical triage artifacts live at:
- `<triage_root>/<phase_id>/findings/<finding_id>.ttl`

## Triage Modes

### `initial_pass`

Use before any fix has been dispatched for the finding.

Goal:
- correlate the finding across all current worktrees
- identify `highest_open_branch`
- determine `promote_to_branch`
- run the repeatable-pattern sweep on the promoted branch when required

### `followup_pass`

Use after fixes or merge-forward activity have already happened.

Goal:
- compare current branch state with the existing `.ttl` record
- identify:
  - still open
  - propagated
  - merge-forward needed
  - regressed

## Team-Lead Execution Loop

### 1. Launch one `qa-triage` agent per finding

Launch one background `qa-triage` agent per finding. Parallel launch is
expected.

Each agent input must include:
- `triage_mode`
- `phase_id`
- finding metadata
- ordered `worktrees`
- `triage_root`

The worktree ordering is authoritative. Do not infer promotion order from
branch names.

### 2. Wait for triage completion before dev dispatch

Do not assign any fix work until:
- every finding in the batch has a completed `qa-triage` result
- every canonical `.ttl` record exists
- each finding reports `dispatch_ready = true` or a valid non-dispatch result

### 3. Aggregate triage results

Read all per-finding records under:
- `<triage_root>/<phase_id>/findings/*.ttl`

Group findings by:
- `promote_to_branch`

Separate them into:
- open findings requiring dev work
- merge-forward-needed findings
- already-fixed findings
- regressed findings
- non-dispatchable findings

The per-finding `.ttl` record is canonical. Aggregation is derived.

### 4. Dispatch branch-scoped fix work to `arch-ctm`

For each promoted branch with open work:
1. render `.claude/skills/codex-orchestration/fix-assignment.xml.j2`
2. include all findings promoted to that branch
3. include all concrete occurrences found on that branch
4. include triage record paths in the references section
5. send one branch-scoped ATM assignment to `arch-ctm`

Recommended render pattern:

```bash
sc-compose render \
  --root .claude/skills/codex-orchestration \
  --file fix-assignment.xml.j2 \
  --var-file /tmp/fix-vars.json
```

## Dispatch Rules

- `highest_open_branch` owns the fix.
- Lower-branch duplicates are not dispatched separately when a higher open
  branch already owns the work.
- If a finding is fixed on a higher branch but still open below, treat it as a
  merge-forward issue unless triage shows a real regression.
- Repeatable findings must be dispatched with the full promoted-branch sweep
  scope, not just the first reported location.
- `dispatch_ready = false` means do not send the finding to dev yet.

## Closure Rules

QA findings are not closed by `team-lead`.

Use this authority split:
- `qa-triage` updates evidence:
  - occurrence state
  - branch state
  - derived finding status
- `team-lead` routes work and may mark a dispatch batch complete operationally
- `quality-mgr` owns finding closure after follow-up QA confirms the fix

Practical rule:
- triage may mark an occurrence or branch `fixed`
- only `quality-mgr` should treat the finding as closed from the QA workflow

Until a dedicated closeout writer exists, use:
- triage `.ttl` status for correlation and routing
- `quality-mgr` PASS / follow-up QA report as the closure authority

## Phase-End Post-Mortem

At the end of a phase, after QA findings are closed, run the post-mortem review
described in `references/post-mortem.md`.

Participants:
- `team-lead`
- `arch-ctm`
- `quality-mgr`

Purpose:
- review the phase finding set as a whole
- classify recurring patterns
- produce systemic follow-up recommendations such as:
  - new ADRs
  - new lints
  - boundary updates
  - planning-process improvements
  - QA-process improvements

## Reporting to Dev

Send findings to `arch-ctm` only after triage completes.

Each fix assignment must include:
- target branch and worktree
- finding ids
- concise summaries
- all promoted-branch occurrences
- whether the issue is repeatable
- whether merge-forward handling is part of the task
- required validation

Do not send:
- findings already closed by follow-up QA
- findings with `dispatch_ready = false`
- lower-branch duplicates already subsumed by a higher promoted branch

## ATM Message Contract

Every handoff follows the team protocol:
1. immediate ACK
2. work
3. completion summary
4. completion ACK by receiver

No silent processing.
