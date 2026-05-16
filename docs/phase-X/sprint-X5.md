---
id: X.5
title: Guardrails And Closeout Verification
status: replayed
branch: feature/pXb-s5-guardrails-and-closeout
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pXb-s5-guardrails-and-closeout
target: integrate/phase-Xb
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

## Replay Status

- replayed on `feature/pXb-s5-guardrails-and-closeout` at `5d4c92a`
- replayed from prior Phase `X` salvage commits:
  - `78d1e2c`
  - `c8bd38a`
  - `cdb1edd`
- `quality-mgr` alignment review already confirmed the replayed branch matches
  the intended sprint deliverables at a non-critical level

## Already Complete On Restart Branch

- the closeout guardrail scripts are already present on the replayed branch
- the replayed branch already builds and passes `cargo test --workspace`
- the typed-observability/process baseline from `TASK-1515` remains present on
  the restart line

## Remaining Restart Work

- remove the unused dev-dependencies causing `cargo-shear` failure:
  - `crates/atm-core/Cargo.toml` dev-dependency on `atm-rusqlite`
  - `crates/atm-daemon/Cargo.toml` dev-dependency on
    `atm-runtime-test-support`
- remove the forbidden `atm-core -> atm-rusqlite` dev edge so the boundary
  lint passes without exceptions
- split `crates/atm-daemon/src/reconcile_runtime.rs:reconcile` below the
  RULE-002 `80`-line limit
- split
  `crates/atm-rusqlite/src/mailbox_metadata.rs:query_mailbox_metadata_rows`
  below the RULE-002 `80`-line limit
- keep `atm_core::boundary_support` as a contained hidden daemon-support seam
  only while resolving the remaining closeout debt; do not add new consumers
  outside the daemon-owned direct-boundary path
- rerun the full restart closeout validation on
  `feature/pXb-s5-guardrails-and-closeout`:
  - `python3 .just/run_lint.py all`
  - `cargo build --workspace`
  - `cargo test --workspace`
  - `cargo clippy --workspace -- -D warnings`
- this sprint owns the remaining replayed full-stack lint closure unless a
  lower sprint must be reopened to preserve independent validation

## Exact Targets

- `scripts/check-legacy-mailbox-paths.py`
- `scripts/check-capability-degradation.py`
- `crates/atm-core/Cargo.toml`
- `crates/atm-daemon/Cargo.toml`
- `crates/atm-daemon/src/reconcile_runtime.rs`
- `crates/atm-rusqlite/src/mailbox_metadata.rs`
- `crates/atm-core/src/lib.rs`
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
- verify the replayed branch does not broaden hidden compatibility seams while
  fixing manifest and lint debt
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
- the closeout branch does not add new non-daemon consumers of
  `atm_core::boundary_support`
- the `TASK-1515` baseline artifacts remain present and consistent at Phase `X`
  closeout
  - `docs/requirements.md` typed observability migration requirement
  - `docs/architecture.md` phased typed observability migration note
  - rust QA checklist infallible-result review step
  - Rust development guidelines daemon structured-logging advisory
- deletion-sprint QA instructions explicitly require whole-workspace pattern
  searches for removed legacy constructs

## Required Validation

- execute each new script locally in its intended mode
- run the affected CI/lint entrypoints locally, or record any unavailable
  entrypoint in the sprint validation report
- `python3 .just/run_lint.py all`
- run `cargo-shear`
- `git diff --check`
