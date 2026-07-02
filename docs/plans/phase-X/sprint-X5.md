---
id: X.5
title: Sprint X.5 — Guardrails, Dependency Ownership, And Closeout Verification
status: complete
branch: feature/pXb-s5-guardrails-and-closeout
worktree: ../atm-core-worktrees/feature/pXb-s5-guardrails-and-closeout
target: integrate/phase-Xb
---

# Sprint X.5 — Guardrails, Dependency Ownership, And Closeout Verification

## Modification

- This sprint is a restart replay on `feature/pXb-s5-guardrails-and-closeout`.
- Prior Phase `X` already completed the main `X.5` implementation and two
  follow-up fix rounds:
  - `78d1e2ceb3ae8862b5179408090e0b65ac2fb07c`
    - `feat: complete phase X guardrails and closeout`
  - `c8bd38a6561623276b3b7bc0874417757b64dab6`
    - `fix: close phase X5 follow-up findings`
  - `cdb1edd2b215afb3a489ae335fce7d9279bfa53f`
    - `fix: close phase X follow-up queue`
- Replay this sprint selectively after audit. Do not blindly replay the old
  branch head because it also contains merge-repair and cross-sprint carry
  forward noise.
- QA must validate the entire `X.5` sprint on `pXb-s5`, not only the replayed
  delta from those prior commits.

## Remaining Restart Work

- none for core sprint scope; `feature/pXb-s5-guardrails-and-closeout`
  is the replayed and validated `X.5` baseline
- only between-sprint finding fixes remain when new `X.5` findings are
  promoted to this branch
- run full `X.5` QA on `pXb-s5` after each promoted finding fix

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
  prerequisites, not delayed `integrate/phase-Xb` sprint work
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

### Closed Findings

- `PLAN-GAP-001`
  - `crates/atm-core/Cargo.toml`
  - closed by `069be8c`
  - removed the unused `atm-rusqlite` dev-dependency so the replayed
    `X.5` branch no longer carries stale dependency-ownership debt
- `PLAN-GAP-002`
  - `crates/atm-core/Cargo.toml`
  - closed by `069be8c`
  - removing the same unused dev-dependency also eliminated the forbidden
    `atm-core -> atm-rusqlite` boundary edge that was still failing
    closeout validation
- `ATM-QA-S4-001`
  - `crates/atm-daemon/src/peer_transport.rs`
  - `PeerTransportConfig::from_config(...)` now emits `tracing::warn!` before
    falling back to `DEFAULT_REMOTE_RETRY_BUDGET` when no config is provided
- `FTQ-006`
  - `crates/atm-daemon/src/composition.rs`
  - `CwdGuard` with a `Drop` restore path now replaces every manual
    `set_current_dir(...)` / restore pair in the touched tests
- `RBP-F005`
  - `crates/atm-daemon/src/local_ipc_transport.rs`
  - `serve_with_deadlines_and_accept_probe(...)`, the extracted request worker,
    and the touched reconcile runtime paths are all kept at or below the
    `RULE-002` threshold on the replayed `X.5` branch

## Required Validation

- execute each new script locally in its intended mode
- run the affected CI/lint entrypoints locally, or record any unavailable
  entrypoint in the sprint validation report
- run `cargo-shear`
- `git diff --check`
