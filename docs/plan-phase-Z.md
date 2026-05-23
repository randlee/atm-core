# Phase Z Plan

## Goal

Validate the first daemon + SQLite mail-SSOT release in real executable use
after the `Phase Y` implementation line closes and the final `develop` gate is
explicitly opened.

Phase `Z` owns the progressive rollout and release-readiness work that should
not be mixed into the architectural cleanup history:

- daemon bring-up on the real binaries
- executable smoke coverage across the supported feature set
- smoke finding closure and revalidation
- Claude roster / config / restore ownership hardening needed before broader
  dogfood
- `atm-dev` canary / dogfood on the new executables
- final release-fix loop and ship/no-ship verdict

## Baseline

- planning branch: `plan/phase-Z`
- follow-up planning branch for post-`Z.1` fix planning: `plan/phase-Z-fix-planning`
- prerequisite implementation line:
  - `Phase Y` accepted through the final `Phase Yd` develop gate
  - `Phase Ye` closed on `develop`
- blocking closeout line before `Phase Z` may begin:
  - `Phase Yd`
- execution integration branch: `integrate/phase-Z`

## Phase Entry Criteria

`Phase Z` does not begin until the accepted `Phase Y` line is develop-ready and
the final `Phase Yd` record says `Phase Z` may begin:

- the write-owner boundary is enforced
- the delivery-policy coordinator and required state machines are landed
- the compatibility field set is finalized
- the append-only/export contract decision is complete
- the later `Phase Yb` / `Phase Yc` message-path and production-readiness
  follow-up work is closed on the accepted `Phase Y` line
- the blocking issues in `docs/phase-Y/issues.md` are closed
- the readiness record in `docs/phase-Yd/readiness.md` explicitly states:
  - `Phase Y` may land on `develop`
  - `Phase Z` may begin
- the post-`Phase Y` daemon ownership simplification line in `Phase Ye` is
  complete and no longer changes the rollout gate

Current gate status:

- `Phase Yd` final accepted candidate line: `19376e42`
- `Phase Y` may land on `develop`
- `Phase Z` may begin
- `Phase Ye` is complete and merged on the current `develop` baseline

## Pre-Phase JSON I/O Status

The CLI JSON I/O audit is already complete:

- audit record: `docs/phase-Z/cli-json-io-audit.md`
- retained-command `--json` output is already implemented on all 9 commands
- no `Phase Y` or `Phase Z` output retrofit work is required
- structured JSON input remains absent and is explicitly deferred until after
  `Phase Z`

The planning consequence is intentional:

- `Phase Z` is not blocked on a JSON-output expansion sprint
- `Phase Z` smoke/dogfood should validate the existing public JSON outputs as
  part of normal executable coverage
- any future JSON-input work must start from a separate public DTO design and
  must not be smuggled into the smoke/release validation line

## Sprint Sequence

### Z.1 Smoke Bring-Up

Purpose:

- developer-coordinated daemon bring-up
- feature-by-feature executable smoke pass
- corner-case and recovery verification on the real binaries
- freeze the authoritative smoke checklist and smoke findings ledger used by
  `Z.2`

Execution branch:
- `feature/pZ-s1-smoke-bring-up`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s1-smoke-bring-up`

### Z.2 Fix And Revalidate

Purpose:

- close smoke findings from `Z.1`
- re-run full executable validation on the fixed branch
- carry forward only the frozen `Z.1` smoke findings ledger
- next-unused execution sprint after completed `Z.1`

Execution branch:
- `feature/pZ-s2-fix-and-revalidate`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s2-fix-and-revalidate`

### Z.5 Runtime Roster Truth Cutover

Purpose:

- remove retained runtime `config.json` roster-truth reads from `list`,
  `read`, `clear`, and `ack`
- keep `doctor` as the explicit config-vs-ATM comparison surface

Execution branch:
- `feature/pZ-s5-runtime-roster-truth-cutover`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s5-runtime-roster-truth-cutover`

### Z.6 Claude Send Semantics And Immutable Runtime Roster View

Purpose:

- land the accepted post-write Claude send warning semantics
- introduce immutable `ClaudeCodeTeamRoster`
- remove generic runtime `load_team_config(...)` use from the send/runtime
  helper surface

Execution branch:
- `feature/pZ-s6-claude-send-semantics-and-runtime-roster-view`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s6-claude-send-semantics-and-runtime-roster-view`

### Z.7 Config Ingress Boundary Narrowing And Static Gates

Purpose:

- narrow `ConfigIngress` so it is no longer a generic runtime roster lookup
- define repo-local lint / `sc-lint`-candidate gates for `config.json`
  boundary violations

Execution branch:
- `feature/pZ-s7-config-ingress-boundary-narrowing-and-static-gates`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s7-config-ingress-boundary-narrowing-and-static-gates`

### Z.8 Watcher-Owned Claude Config Ingest

Purpose:

- make watcher / reconcile the only roster-truth reader of `config.json`
- import new-team and external config changes into canonical ATM roster state

Execution branch:
- `feature/pZ-s8-watcher-owned-claude-config-ingest`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s8-watcher-owned-claude-config-ingest`

### Z.9 Team Admin Roster Authority And Member Metadata

Purpose:

- move `atm members` / `atm teams` to ATM roster truth
- make ATM member-add the canonical mutation path
- move retained Claude member metadata such as `tmux_pane_id` into canonical
  ATM roster ownership

Execution branch:
- `feature/pZ-s9-team-admin-roster-authority-and-member-metadata`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s9-team-admin-roster-authority-and-member-metadata`

### Z.10 Team Backup Restore Automation And Config Projection

Purpose:

- keep raw backup snapshots for audit value
- make restore rebuild recreated Claude team config from canonical ATM roster
  truth
- replace manual restore-file surgery with ATM-owned projection
- after `Z.10`, execution resumes on the already-defined canary / release sprints
  `Z.3` and `Z.4`; those sprint numbers are retained to preserve the original
  rollout identities

Execution branch:
- `feature/pZ-s10-team-backup-restore-automation-and-config-projection`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s10-team-backup-restore-automation-and-config-projection`

### Z.11 First Send Recovery Contract And Setup Guidance

Purpose:

- replace the bad clean-start first-send failure with explicit operator
  guidance
- keep the empty-roster first-send path actionable without adding hidden
  fallback behavior

Execution branch:
- `feature/pZ-s11-first-send-recovery-contract-and-setup-guidance`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s11-first-send-recovery-contract-and-setup-guidance`

### Z.12 Retained Runtime Path Elimination And Boundary Lint Gate

Purpose:

- eliminate the incorrect retained-runtime acquisition path behind `Z2-F001`
- make `atm teams add-member` use that same approved runtime-entry path
- add a repository-local boundary lint gate so direct CLI-command
  `default_runtime()` misuse cannot return

Execution branch:
- `feature/pZ-s12-retained-runtime-path-elimination-and-boundary-lint-gate`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s12-retained-runtime-path-elimination-and-boundary-lint-gate`

### Z.13 Workspace Config Boundary Cleanup And Lint Gate

Purpose:

- remove ambient `.atm.toml` / `load_config(...)` reads from command/team-admin
  paths
- add a repository-local boundary lint gate so workspace-config access stays
  behind the approved seam

Execution branch:
- `feature/pZ-s13-workspace-config-boundary-cleanup-and-lint-gate`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s13-workspace-config-boundary-cleanup-and-lint-gate`

### Z.14 Ambient Singleton Surface Removal And Lint Gate

Purpose:

- remove the broad public ambient runtime-factory/singleton exposure
- add a repository-local lint gate so that class of surface cannot leak back
- keep only the approved bounded wrappers for daemon bootstrap and
  runtime-test support

Execution branch:
- `feature/pZ-s14-ambient-singleton-surface-removal-and-lint-gate`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s14-ambient-singleton-surface-removal-and-lint-gate`

### Z.3 `atm-dev` Canary And Dogfood

Purpose:

- move from single-operator smoke to `atm-dev` team use on the new binaries
- verify UX, recovery text, and operational behavior under real use
- produce the canary participant list, operator-report path, and canary
  findings ledger used by `Z.4`

Execution branch:
- `feature/pZ-s3-atm-dev-canary-and-dogfood`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s3-atm-dev-canary-and-dogfood`

### Z.4 Final Fixes And Release Sign-Off

Purpose:

- close `Z.3` findings
- produce the final release-readiness verdict
- rerun the final executable validation and release checklist on the closeout
  branch

Execution branch:
- `feature/pZ-s4-final-fixes-and-release-sign-off`

Execution worktree:
- `../atm-core-worktrees/feature/pZ-s4-final-fixes-and-release-sign-off`

## Sprint Artifact Summary

`Phase Z` uses one named artifact set throughout execution:

- `Z.1` / `Z.2`:
  - `docs/phase-Z/smoke-checklist.md`
  - `docs/phase-Z/smoke-findings-ledger.md`
- `Z.5` / `Z.6` / `Z.7` / `Z.8` / `Z.9` / `Z.10`:
  - `docs/phase-Z/claude-roster-sync-and-restore.md`
  - `docs/phase-Z/config-json-violation-inventory.md`
  - `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- `Z.3`:
  - `docs/phase-Z/canary-dogfood-checklist.md`
  - `docs/phase-Z/canary-findings-ledger.md`
- `Z.4`:
  - `docs/phase-Z/release-checklist.md`
  - `docs/phase-Z/readiness.md`

The sprint docs remain the only authoritative source for per-sprint
deliverables, acceptance criteria, and closure rules.

## Phase Rules

- all validation is against the real built executables, not only harness/unit
  tests
- smoke findings feed only the immediately following fix sprint
- the roster/config/restore follow-on line (`Z.5` through `Z.10`) must close
  before `atm-dev` canary use begins
- dogfood findings feed only the final fix/sign-off sprint
- release readiness is not declared until the documented executable flows and
  recovery behavior are revalidated after each fix round

Current execution state:

- `Z.1` is complete on `feature/pZ-s1-smoke-bring-up @ 70f4fa7f`
- `Z.1` froze the smoke artifacts and promoted exactly two blocking findings to
  `Z.2`:
  - `Z1-F001` first-team roster bootstrap / harness-resolution gap
  - `Z1-F002` preexisting-db sqlite schema-init ordering failure
- `Z.2` is the next-unused sprint and is limited to those findings plus frozen
  checklist revalidation
- the broader roster/config/restore ownership redesign discovered while
  analyzing `Z1-F001` is split into `Z.5` through `Z.10`
- the remaining boundary-cleanup line is now explicitly split into:
  - `Z.11` first-send recovery contract
  - `Z.12` retained runtime path cleanup
  - `Z.13` workspace-config boundary cleanup
  - `Z.14` ambient singleton surface cleanup
- `Z.3` and `Z.4` remain the canary / release sprints, but execution does not
  resume there until `Z.10` closes

## Initial Planning Outputs

- `docs/plan-phase-Z.md`
- `docs/adr/ADR-016-claude-config-ingress-and-roster-projection-ownership.md`
- `docs/phase-Z/cli-json-io-audit.md`
- `docs/phase-Z/claude-roster-sync-and-restore.md`
- `docs/phase-Z/config-json-violation-inventory.md`
- `docs/phase-Z/smoke-checklist.md`
- `docs/phase-Z/smoke-findings-ledger.md`
- `docs/phase-Z/canary-dogfood-checklist.md`
- `docs/phase-Z/canary-findings-ledger.md`
- `docs/phase-Z/release-checklist.md`
- `docs/phase-Z/readiness.md`
- `docs/phase-Z/sprint-Z1.md`
- `docs/phase-Z/sprint-Z2.md`
- `docs/phase-Z/sprint-Z5.md`
- `docs/phase-Z/sprint-Z6.md`
- `docs/phase-Z/sprint-Z7.md`
- `docs/phase-Z/sprint-Z8.md`
- `docs/phase-Z/sprint-Z9.md`
- `docs/phase-Z/sprint-Z10.md`
- `docs/phase-Z/sprint-Z3.md`
- `docs/phase-Z/sprint-Z4.md`
