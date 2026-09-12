---
name: codex-orchestration
version: 0.1.0
description: Orchestrate atm-core sprint work where an appointed lead coordinates, arch-ctm is the sole developer, and quality-mgr enforces the QA gate.
depends_on:
  quality-management-gh: 1.x
  quality-mgr: 0.x
  req-qa: 0.x
  arch-qa: 0.x
  flaky-test-qa: 0.x
  ruthless-boundary-qa: 0.x
  rust-qa-agent: 0.x
  rust-best-practices-agent: 0.x
  rust-service-hardening-agent: 0.x
---

# Codex Orchestration

This skill defines the repo-local orchestration workflow for `atm-core`.

## Model

- The **lead** coordinates sprint sequencing, worktree assignments, PR flow,
  and every dispatch and report in this skill. `team-lead` is the default
  lead; `fenix` or any other identity may hold the role.
- `arch-ctm` is the sole developer for Codex-driven implementation work
- `quality-mgr` runs the QA gate after each delivery

## Lead Role

- A lead must be appointed for every phase before its first dispatch. Record
  the appointment in the phase status row of `docs/project-plan.md` and in the
  phase ledger, and announce it over ATM to every agent in the roster.
- Every template in this skill addresses the lead through the `lead`
  variable (default `team-lead`). Dispatch with `--var lead=<identity>` or a
  `lead` key in the vars file so acks, push reports, and closes reach the
  identity that actually holds the role; never hard-code `team-lead`.
- The role can be transferred mid-phase. The outgoing lead sends the incoming
  lead a handoff message listing open task ids, open PRs, and pending QA
  rounds, records the transfer in the ledger, and announces the new lead to
  the roster. In-flight tasks keep their original assigner; the new lead
  reads those reports via `atm read --task <task-id>`. Tasks dispatched after
  the transfer name the new lead.

Reports carbon-copy `team-lead` by default. Every template also takes a `cc`
variable (default `team-lead`): when the lead is another identity, the
assignee sends a one-line plain copy of each push report and close summary
to `cc`; when `lead` and `cc` are the same identity, nothing extra is sent.
The lead may set `cc` to an empty string to switch copies off.

## Preconditions

Before starting a sprint:
1. `docs/requirements.md`, `docs/architecture.md`, and `docs/project-plan.md`
   define the sprint or phase review target.
2. A worktree exists for the sprint branch under the repo’s worktree strategy.
3. The target branch for the sprint is chosen from the current repo plan.
4. The following prompts exist in `.claude/agents/`:
   - `quality-mgr.md`
   - `req-qa.md`
   - `arch-qa.md`
   - `flaky-test-qa.md`
   - installed Rust reviewers from `sc-rust`
5. The following QA reporting skill exists in `.claude/skills/`:
   - `quality-management-gh/`
6. `quality-mgr` must read:
   - `.claude/assets/sc-rust/quality-mgr/quality-mgr.rust.md`
7. `quality-mgr` must also read:
   - `.claude/skills/quality-management-gh/SKILL.md`
8. Every ATM assignment is sent with
   `atm send <agent> --task-id "$TASK_ID" --template <template> --vars <json>`;
   the same `TASK_ID` is supplied as the template's `task_id` variable. Never
   render a template yourself and send the output as message text or via `--stdin`.
   To view or validate the exact body before sending, use
   `atm compose --template <template> --vars <json>` (same renderer, same
   vars). The template path goes through the daemon-owned admission path
   and the dispatch is queryable from outside.
9. `.claude/agents/ruthless-boundary-qa.md` and
   `.claude/skills/codex-orchestration/ruthless-boundary-qa-assignment.json.j2`
   exist for first-pass boundary optimization review.

## Sprint Flow

1. the lead assigns development to `arch-ctm` using `dev-template.xml.j2`.
   Every dev assignment must include the sprint-plan document path as
   `sprint_doc`, and that sprint document is the authoritative source for the
   task. Assignment prose may summarize, but it must not replace or weaken the
   sprint doc.
2. `arch-ctm` ACKs, implements, commits, pushes, and reports branch plus SHA.
3. Before QA-1, `arch-ctm` performs a self-directed Rust best-practices sweep on
   the integration branch using the same `review_targets` planned for QA-1 and
   fixes all RBP findings found there. This is a developer cleanup step, not a
   QA surprise.
4. the lead opens or updates the PR.
5. the lead assigns QA to `quality-mgr` using `qa-template.xml.j2`.
   Every QA assignment must include `sprint_doc`, and `quality-mgr` must treat
   that sprint document as the authoritative QA scope source.
6. `quality-mgr` launches the reviewer set:
   - `req-qa`
   - `arch-qa`
   - `ruthless-boundary-qa`
   - `rust-qa-agent`
   - `rust-best-practices-agent`
   - `rust-service-hardening-agent`
   - `flaky-test-qa` when test instability risk is present
   - for the near term, `ruthless-boundary-qa` stays enabled on every sprint
     QA round, plus docs-only plan review and phase-ending review
7. QA-2 and later rounds must omit `rust-best-practices-agent` and
   `rust-service-hardening-agent`. All
   first-pass findings from those reviewers must be fixed before merge —
   merge gate is 0B+0I+0m with no exceptions and no backlog deferral. QA-1
   findings route back to `arch-ctm` via `fix-assignment.xml.j2` before
   QA-2, following the standard triage-and-fix path.
   `ruthless-boundary-qa` remains part of that loop unless the lead
   explicitly narrows the reviewer set for a specific task.
8. If QA passes and CI is green, merge may proceed.
9. If QA fails, the lead first runs `/triaging-findings` to correlate the
   findings across worktrees and determine the promoted fix branch.
10. After triage completes, the lead routes concrete fixes back to
   `arch-ctm` using `fix-assignment.xml.j2`. Fix assignments must also include
   `sprint_doc`, and the sprint document remains authoritative if the task
   summary omits or compresses details.

## Stacked Phases

When a phase runs as a `gh stack` of sprint and fix layers above
`integrate/phase-N`, the stack rules in
[`docs/development/gh-stack-guidelines.md`](../../../docs/development/gh-stack-guidelines.md)
govern layer ownership, rebase-at-task-start, freezing, PR-on-first-push,
the CI-trigger PR, and the landing sequence. The orchestrator owns the stack;
each dev owns exactly one layer. Fix and cleanup work goes to a new top layer
with one QA pass, never to a frozen layer.

## Plan Review Flow

1. the lead completes `/plan-hardening` steps 1 through 5.
2. the lead assigns plan QA to `quality-mgr` using `qa-template.xml.j2`
   with `review_mode: plan`.
3. The QA assignment must include the phase-plan document as `sprint_doc`, and
   that plan document is the authoritative scope source for plan QA.
4. `quality-mgr` treats `review_mode: plan` as docs-only review and launches:
   - `req-qa`
   - `arch-qa`
   - `ruthless-boundary-qa`
   - `rust-best-practices-agent`
   - `rust-service-hardening-agent`
5. If plan QA passes, the hardened plan is ready for implementation dispatch.
6. If plan QA fails, the lead uses the normal codex-orchestration
   triage-and-fix loop to route concrete fixes back to `arch-ctm`.

## QA Coverage Rule

- `quality-mgr` must extract every deliverable, acceptance criterion, deletion
  target, required validation item, and expected artifact from `sprint_doc`
  before launching `req-qa`
- `req-qa` must independently treat `sprint_doc` as authoritative
- `req-qa` must count deliverable completion and report a completion percentage
- `arch-qa` must inspect sprint-doc structural gate artifacts directly when a
  deliverable points to a boundary, packaging, release-tracking, readiness, or
  validation gate
- QA cannot PASS unless deliverable completion is 100%

## Phase-End Review

For extraction-readiness or phase-close reviews, use `review-template.xml.j2`
to assign a read-only review to `arch-ctm`.
After the phase lands, run the `triaging-findings` post-mortem
(`.claude/skills/triaging-findings/references/post-mortem.md`); the write-up
lives in `docs/postmortems/` and feeds the stack guidelines above.

For phase-ending QA routed through `quality-mgr`, the reviewer set is
mandatory:
- `req-qa`
- `arch-qa`
- `ruthless-boundary-qa`
- `rust-qa-agent`
- `rust-best-practices-agent`
- `rust-service-hardening-agent`
- `flaky-test-qa`

Before phase-ending QA can report PASS, `quality-mgr` must require a successful
`just validate` result from the assigned execution reviewer (normally
`rust-qa-agent`). To preserve `quality-mgr`'s no-foreground-QA rule, it verifies
the delegated `executed_checks.artifacts` output and source revision instead of
executing that broad command itself. The phase-end rust-qa assignment supplies
`just validate` through its artifact-command channel. `just validate` includes
the committed CLI-surface contract check, so released CLI changes cannot bypass
its baseline gate.

## CI

Use standard GitHub CLI:
- `gh pr checks <PR> --watch`
- `gh pr view <PR> --json mergeStateStatus,reviewDecision`

Do not assume ATM-specific PR monitoring commands exist.

## Assignment Templates

Dispatch form (mandatory for every assignment below):

```bash
TASK_ID="<task-id>"
atm send <agent> \
  --task-id "$TASK_ID" \
  --template <path/to/template.j2> \
  --vars <vars.json> \
  --var task_id="$TASK_ID"
```

Install the repository templates on the daemon host after this change merges:

```bash
mkdir -p ~/.atm/templates/codex-orchestration && cp .claude/skills/codex-orchestration/*.j2 ~/.atm/templates/codex-orchestration/
```

On atm 1.5.16, closing an assignee task with its final report also closes the
assigner's mirror task. The lead must not issue a second `--task-complete` for
that mirror; it reads the assignee's close report and proceeds with QA or the
next orchestration step.

Use the templates in this skill directory:
- `dev-template.xml.j2`
- `fix-assignment.xml.j2`
- `qa-template.xml.j2`
- `review-template.xml.j2`
- `req-qa-assignment.json.j2`
- `arch-qa-assignment.json.j2`
- `flaky-test-qa-assignment.json.j2`
- reporting templates under `.claude/skills/quality-management-gh/`

Use the Rust assignment templates from:
- `.claude/assets/sc-rust/quality-mgr/templates/`

## Required Message Sequence

Every ATM task message must follow:
1. ACK
2. Work
3. Completion summary
4. Completion ACK by receiver
