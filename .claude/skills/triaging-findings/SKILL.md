---
name: triaging-findings
version: 1.1.0
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
- `integration_branch`
- `integration_worktree_path`
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

Required ownership rule:
- `triage_root` must live under `integration_worktree_path`
- the phase integration worktree is the canonical source of truth for triage
  artifacts

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
- `integration_branch`
- `integration_worktree_path`
- finding metadata
- ordered `worktrees`
- `triage_root`

The worktree ordering is authoritative. Do not infer promotion order from
branch names.

### 2. Wait for triage completion before dev dispatch

Do not assign any fix work until:
- every finding in the batch has a completed `qa-triage` result
- every canonical `.ttl` record exists
- every `qa-triage` result has been checked for
  `dispatch_blocked_pending_triage_commit`
- each finding reports `dispatch_ready = true` or a valid non-dispatch result
- dispatch remains blocked when any `qa-triage` result reports
  `dispatch_blocked_pending_triage_commit = true`

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

### 3.1 Commit triage artifacts before dispatch

After all `qa-triage` agents in the batch have finished and after aggregation
confirms the `.ttl` set is complete, stage and commit the triage artifacts to
git before sending any dev assignment to `arch-ctm`.

Required commit scope:
- the phase findings under `<triage_root>/<phase_id>/findings/`
- any phase-local triage metadata needed for later follow-up, such as
  worktree inventories under `<triage_root>/<phase_id>/`

Required timing:
- after triage batch aggregation
- before branch-scoped fix dispatch
- on the integration-branch worktree that is the canonical triage source of
  truth for the phase

`triage_root` must point to the integration-branch worktree for the active
phase, not a feature branch or main-repo path.

Reason:
- parallel `qa-triage` agents write into one shared triage root
- committing inside each agent would create batch races and partial evidence
- leaving `.ttl` records untracked until phase end risks silent loss of the
  canonical QA evidence

Do not dispatch dev work from uncommitted `.ttl` state.

The per-finding `.ttl` record is canonical. Aggregation is derived.

### 3.1 Commit triage artifacts before dispatch

After all `qa-triage` agents in the batch have finished and after aggregation
confirms the `.ttl` set is complete, stage, commit, and push the triage
artifacts to git before sending any dev assignment to `arch-ctm`.

Required commit scope:
- the phase findings under `<triage_root>/<phase_id>/findings/`
- any phase-local triage metadata needed for later follow-up, such as
  worktree inventories under `<triage_root>/<phase_id>/`

Required timing:
- after triage batch aggregation
- before branch-scoped fix dispatch
- on the phase integration-branch worktree identified by
  `integration_branch` / `integration_worktree_path`

Reason:
- parallel `qa-triage` agents write into one shared triage root
- committing inside each agent would create batch races and partial evidence
- leaving `.ttl` records untracked until phase end risks silent loss of the
  canonical QA evidence

Do not dispatch dev work from uncommitted `.ttl` state.

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

For follow-up QA or reviewer rechecks, build the carry-forward payload from the
same `.ttl` records instead of handcrafting it:

```bash
python3 scripts/triage_carry_forward.py \
  --branch <branch> \
  --ttl <triage_record_1.ttl> \
  --ttl <triage_record_2.ttl>
```

Use the script output as the `carry_forward_findings_json` template input.

Prompt/handoff contract:
- `qa-triage` itself is a JSON-in / fenced-JSON-out agent prompt
- ATM task assignment templates remain XML ATM messages
- when dispatching work, pass triage record paths or rendered carry-forward JSON
  rather than copying raw `.ttl` contents into the task body

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

At the end of a phase, after all sprint branches are integrated into
`integrate/phase-X` and before the final merge to `develop`, run the
post-mortem review described in `references/post-mortem.md`.

Participants:
- `team-lead`
- `arch-ctm`
- `quality-mgr`

Purpose:
- review the phase finding set as a whole
- run one final `integrate/phase-X` quality gate
- classify recurring patterns
- produce systemic follow-up recommendations such as:
  - new ADRs
  - new lints
  - boundary updates
  - planning-process improvements
  - QA-process improvements

Required gate:
- `quality-mgr` must run a full review on `integrate/phase-X`
- `quality-mgr` should deploy a background review team by role for that final
  pass, using the appropriate reviewer mix for the phase artifacts
- that review must verify 100% of phase findings are fixed or intentionally
  deferred on the integration branch
- that review team must verify no integrated fix was missed outside the original
  changed-file scopes
- do not merge `integrate/phase-X` to `develop` until that review passes

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
