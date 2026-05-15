---
id: X.5
title: Guardrails And Closeout Verification
status: complete
branch: feature/pX-s5-guardrails-and-closeout
worktree: ../atm-core-worktrees/feature/pX-s5-guardrails-and-closeout
target: integrate/phase-X
---

# Sprint X.5 — Guardrails, Dependency Ownership, And Closeout Verification

## Goal

- finish the remaining mechanical guardrails after the structural deletion
  sprints land
- verify the already-landed typed-observability/process baseline from
  `TASK-1515` remains present and aligned at Phase `X` closeout

## Hard Dependencies

- `X.0` merged on `develop`
- `X.1` through `X.4` complete because this sprint validates the post-cutover
  guardrails against the final Phase `X` implementation shape

## Exact Targets

- `scripts/check-legacy-mailbox-paths.py`
- `scripts/check-capability-degradation.py`
- CI workflow files that own repository gate execution
- `.claude/assets/sc-rust/quality-mgr/templates/`
- `.claude/skills/rust-development/guidelines.txt`
- verification targets already landed on the baseline:
  - `docs/requirements.md`
  - `docs/architecture.md`
  - `.claude/skills/codex-orchestration/dev-template.xml.j2`
  - `.claude/skills/codex-orchestration/qa-template.xml.j2`

## Required Work

- add a CI gate for mailbox-legacy deletion regressions
- add a CI gate preventing replay-capability degradation regressions after
  `X.4`
- wire dependency-ownership validation, including `cargo-shear`, into the
  active lint/CI path
- update QA/checklist language so deletion sprints require whole-workspace
  pattern searches for removed legacy constructs
- verify the already-landed `TASK-1515` baseline remains present and aligned:
  - typed observability requirement in `docs/requirements.md`
  - phased typed observability note in `docs/architecture.md`
  - infallible-result review step in the rust QA checklist
  - daemon structured-logging guidance in Rust development guidance

## Acceptance Criteria

- the legacy-mailbox-regression gate is runnable in CI
- the replay-capability-degradation regression gate is runnable in CI
- the pre-phase silent-emit and RULE-002 gates are treated as already-live
  prerequisites, not delayed `integrate/phase-X` sprint work
- the local lint entrypoints include dependency-ownership validation
- the `TASK-1515` baseline artifacts remain present and consistent at Phase `X`
  closeout
  - `docs/requirements.md` typed observability migration requirement
  - `docs/architecture.md` phased typed observability migration note
  - rust QA checklist infallible-result review step
  - Rust development guidelines daemon structured-logging advisory
- deletion-sprint QA instructions explicitly require whole-workspace pattern
  searches for removed legacy constructs

## Delivered

- added Phase `X` closeout guardrail scripts:
  - `scripts/check-legacy-mailbox-paths.py`
  - `scripts/check-capability-degradation.py`
- wired those gates into the active local lint surface:
  - `Justfile`
  - `.just/run_lint.py`
  - `.just/print_help.py`
  - `.just` unit coverage for the new entries
- fixed the RULE-002 helper to ignore trait method declarations so the
  function-length gate only evaluates real function bodies
- updated QA/checklist language so deletion sprints must search the full
  workspace for each removed legacy construct family, not only touched files
- verified the carried baseline from `TASK-1515` remains present while adding
  the new deletion and dependency-ownership closeout checks
- moved the production retained-runtime factory install edge into
  `atm-daemon-bootstrap` and removed the stale direct production
  `atm -> atm-rusqlite` dependency
- added `atm-runtime-test-support` for SQLite retained-runtime test assembly,
  including a process-visible runtime-path guard that works for spawned threads
  without deadlocking multi-fixture tests

## Required Validation

- execute each new script locally in its intended mode
- run the affected CI/lint entrypoints locally, or record any unavailable
  entrypoint in the sprint validation report
- run `cargo-shear`
- `git diff --check`
