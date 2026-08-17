---
id: AD.19-win
title: Native Windows Execution For AD.19
status: completed
branch: feature/pAD-s19-read-mutation-output-consistency-repair
worktree: ../atm-core-worktrees/feature/pAD-s19-read-mutation-output-consistency-repair
target: integrate/phase-AD
---

# Sprint AD.19-win — Native Windows Execution For AD.19

## Goal

- execute `AD.19` on a real Windows host
- capture and fix any Windows-only defect that blocks closure of `AD.19`

## Overlay Rule

- `docs/plans/phase-AD/sprint-AD19.md` remains the source of truth for
  `AD.19` scope, interfaces, paths to delete, deliverables, and acceptance
- this `*-win` doc adds only native Windows execution, Windows-only
  fix-forward, and Windows evidence/reporting requirements for the already
  defined `AD.19` sprint

## Hard Dependencies

- `docs/plans/phase-AD/sprint-AD19.md`
- latest accepted `integrate/phase-AD` planning-doc updates merged into this
  same worktree before Windows execution starts

## Exact Targets

- `docs/plans/phase-AD/sprint-AD19.md`
- `docs/plans/phase-AD/sprint-AD19-win.md`
- any file already in `AD.19` scope that needs Windows-only fix-forward to
  close `AD.19`

## Deliverables

- native Windows execution report for `AD.19`
- any Windows-only fix-forward required to make `AD.19` pass its original
  validation scope on Windows

## Acceptance Criteria

- the Windows agent reads `sprint-AD19.md` first, then this doc
- the Windows agent works only on the `AD.19` branch/worktree named above
- the Windows agent does not widen `AD.19` scope beyond what
  `docs/plans/phase-AD/sprint-AD19.md` already authorizes
- the Windows agent merges forward the latest accepted `integrate/phase-AD`
  tip into that same worktree before running validation
- the Windows agent runs `AD.19`'s original Required Validation on a real
  Windows host where those commands are meaningful
- if a Windows-only defect is found, the fix lands on the same `AD.19`
  worktree and stays within `AD.19` scope
- the final report records exact commands run, pass/fail results, commit hash,
  and any CI run URL used for confirmation

## Required Validation

- all validation from `docs/plans/phase-AD/sprint-AD19.md`
- native Windows execution only; non-Windows cross-compile is not a substitute
- `git diff --check`

## Windows Agent Instructions

1. Read `docs/plans/phase-AD/sprint-AD19.md`.
2. Read `docs/plans/phase-AD/sprint-AD19-win.md`.
3. Treat `sprint-AD19.md` as the governing implementation plan; use this doc
   only for Windows-host execution and reporting additions.
4. Merge forward the latest accepted `integrate/phase-AD` tip into the
   `AD.19` worktree before running anything.
5. Run the original `AD.19` validation commands on the Windows host.
6. If a Windows-only defect appears, fix it on the same `AD.19` worktree
   without widening sprint scope.
7. Re-run the failed validation plus the full original validation set.
8. Push the branch and report the final commit hash, validation results, and
   any CI evidence.

## Windows Execution Report

- Date: 2026-07-07
- Host: native Windows
- Branch: `feature/pAD-s19-read-mutation-output-consistency-repair`
- Worktree:
  `F:\github\atm-core-worktrees\feature\pAD-s19-read-mutation-output-consistency-repair`
- Merge-forward: pulled AD.19 remote tip, merged
  `origin/feature/pAD-s18-raw-cli-runtime-root-unification` into AD.19, and
  confirmed `origin/integrate/phase-AD` was already included.
- Windows fix commit: `173a52ef59466fdec256ebf73457516dd830da09`
- CI evidence: pending remote CI after push.

### Findings And Fixes

- Fixed AD.18-to-AD.19 merge fallout in `crates/atm-core/src/protocol.rs`:
  tests still referenced removed `daemon_socket_file_name`; updated tests to
  use the current `DAEMON_SOCKET_FILENAME` constant while preserving Windows
  legacy local-endpoint normalization.
- Fixed AD.18-to-AD.19 merge fallout in `crates/atm-daemon/src/lib.rs`:
  removed stale flat `tests_runtime_root` module declaration because
  runtime-root tests now live under `tests::runtime_root`.
- Fixed AD.19 read output reload on Windows validation path:
  `output_messages_from_metadata_selection` now reloads the selected durable
  message through the metadata row `message_key` when available, instead of
  assuming every durable key is derivable as `atm:<message_id>`. This preserves
  the AD.19 invariant that the output message is the mutated message and fixes
  compatibility rows whose durable store key differs from the logical message
  id.

### Validation Results

- `cargo test -p agent-team-mail-core
  read_unread_output_stays_consistent_with_the_mutated_message -- --exact`
  passed.
- `cargo test -p agent-team-mail-core
  ack_persists_read_state_and_acknowledged_timestamp -- --exact` passed.
- `cargo test -p agent-team-mail-core --test mailbox_locking
  multi_source_read_and_clear_complete_without_deadlock -- --exact` passed
  after the durable-key reload fix.
- `cargo test --workspace` passed.
- `cargo clippy --workspace -- -D warnings` passed.
- `python .just/run_lint.py all` passed. `python` was used for native Windows
  execution because the Windows `python3` launcher resolves to a different
  interpreter on this host.
- `just test` passed.
- `just smoke normal` passed and updated `reports/smoke/smoke.md` with
  `AD19-READ-OUTPUT-001`.
- `git diff --check` passed.
