---
name: codex-orchestration
version: 0.1.0
description: Orchestrate atm-core sprint work where team-lead coordinates, arch-ctm is the sole developer, and quality-mgr enforces the QA gate.
depends_on:
  quality-management-gh: 1.x
  quality-mgr: 0.x
  req-qa: 0.x
  arch-qa: 0.x
  flaky-test-qa: 0.x
  rust-qa-agent: 0.x
  rust-best-practices-agent: 0.x
  rust-service-hardening-agent: 0.x
---

# Codex Orchestration

This skill defines the repo-local orchestration workflow for `atm-core`.

## Model

- `team-lead` coordinates sprint sequencing, worktree assignments, and PR flow
- `arch-ctm` is the sole developer for Codex-driven implementation work
- `quality-mgr` runs the QA gate after each delivery

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
8. `sc-compose` is available for rendering the JSON and markdown templates.

## Sprint Flow

1. `team-lead` assigns development to `arch-ctm` using `dev-template.xml.j2`.
   Every dev assignment must include the sprint-plan document path as
   `sprint_doc`, and that sprint document is the authoritative source for the
   task. Assignment prose may summarize, but it must not replace or weaken the
   sprint doc.
2. `arch-ctm` ACKs, implements, commits, pushes, and reports branch plus SHA.
3. Before QA-1, `arch-ctm` performs a self-directed Rust best-practices sweep on
   the integration branch using the same `review_targets` planned for QA-1 and
   fixes all RBP findings found there. This is a developer cleanup step, not a
   QA surprise.
4. `team-lead` opens or updates the PR.
5. `team-lead` assigns QA to `quality-mgr` using `qa-template.xml.j2`.
   Every QA assignment must include `sprint_doc`, and `quality-mgr` must treat
   that sprint document as the authoritative QA scope source.
   Every QA assignment must also list task deliverables, acceptance criteria,
   and expected artifacts. If the task omits deliverables, `quality-mgr` must
   immediately report the assignment as incomplete to `team-lead`, continue the
   review against `sprint_doc`, and return a blocking FAIL verdict.
6. `quality-mgr` launches the reviewer set:
   - `req-qa`
   - `arch-qa`
   - `rust-qa-agent`
   - `rust-best-practices-agent` in QA-1 only
   - `rust-service-hardening-agent` when service-runtime review is in scope
   - `flaky-test-qa` when test instability risk is present
7. QA-2 and later rounds must omit `rust-best-practices-agent`. All RBP
   findings from QA-1 must be fixed before merge — merge gate is 0B+0I+0m
   with no exceptions and no backlog deferral.
   QA-1 RBP findings route back to `arch-ctm` via `fix-assignment.xml.j2`
   before QA-2, following the standard triage-and-fix path.
8. If QA passes and CI is green, merge may proceed.
9. If QA fails, `team-lead` first runs `/triaging-findings` to correlate the
   findings across worktrees and determine the promoted fix branch.
10. After triage completes, `team-lead` routes concrete fixes back to
   `arch-ctm` using `fix-assignment.xml.j2`. Fix assignments must also include
   `sprint_doc`, and the sprint document remains authoritative if the task
   summary omits or compresses details.

## QA Coverage Rule

- `quality-mgr` must extract every deliverable, acceptance criterion, deletion
  target, required validation item, and expected artifact from `sprint_doc`
  before launching `req-qa`
- `req-qa` must independently treat `sprint_doc` as authoritative and must not
  assume the hand-authored deliverables list is exhaustive
- `req-qa` must count deliverable completion and report a completion percentage
- `arch-qa` must inspect task-listed structural gate artifacts directly when a
  deliverable points to a boundary, packaging, release-tracking, readiness, or
  validation gate
- a QA assignment that lists 8 of 12 sprint-doc deliverables is incomplete;
  the 4 omitted items must still be reviewed
- QA cannot PASS unless deliverable completion is 100%

## Phase-End Review

For extraction-readiness or phase-close reviews, use `review-template.xml.j2`
to assign a read-only review to `arch-ctm`.

## CI

Use standard GitHub CLI:
- `gh pr checks <PR> --watch`
- `gh pr view <PR> --json mergeStateStatus,reviewDecision`

Do not assume ATM-specific PR monitoring commands exist.

## Assignment Templates

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
