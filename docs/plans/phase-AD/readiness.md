# Phase AD Readiness

## Goal

Track the authoritative closeout state for the `AD.25` through `AD.30`
follow-up line on top of the already accepted `AD.1` through `AD.22`
corrective release work.

Authoring ownership:

- `AD.29` supplies the authoritative post-send smoke evidence
- `AD.30` is the sole sprint allowed to author the `AD.25` through `AD.30`
  close/not-close verdict in this file

Companion closure ledger:

- `.triage/phase-AD/direct-fix-track.md`

Current readiness verdict:

- `release verdict: AD.25 through AD.30 closeout complete on this branch; the authoritative smoke and Windows daemon-depth evidence are both present.`

Current evidence surfaces:

- normal smoke lane: `reports/smoke/smoke.md`
- thorough smoke lane: `reports/smoke/smoke-thorough.md`
- Windows daemon-depth CI lane: GitHub Actions CI run
  [`29044774805`](https://github.com/randlee/atm-core/actions/runs/29044774805)
  for commit `77c30bb3` / PR `#497`, with successful `windows-latest`
  `atm-daemon` coverage for dispatcher panic during shutdown, injected
  accept-error handling, and post-terminate connection rejection

## Sprint Status

| Sprint | Status | Branch | Worktree | Closure Gate |
| --- | --- | --- | --- | --- |
| `AD.25` | `complete` | `feature/pAD-s25-post-send-hook-emitter-live-wiring` | `../atm-core-worktrees/feature/pAD-s25-post-send-hook-emitter-live-wiring` | override rows expose explicit override, disable, and clear/reset semantics with one stable empty-body error contract |
| `AD.26` | `complete` | `feature/pAD-s26-rule001-observability-seam-closure` | `../atm-core-worktrees/feature/pAD-s26-rule001-observability-seam-closure` | `PostSendHookEmitter` and `GraftPostSendPort` are live seams, mixed-success accounting is real, and the `RULE-001` library-internal adapter exception is closed through `ADR-020` plus lint enforcement so only the sanctioned daemon adapter module keeps the direct observability imports |
| `AD.27` | `complete` | `feature/pAD-s27-upstream-built-in-template-resolution` | `../atm-core-worktrees/feature/pAD-s27-upstream-built-in-template-resolution` | `ADR-019-EXC-AD26-001` is closed and built-in override lookup is upstream of `PostSendHookEmitter` |
| `AD.28` | `complete` | `feature/pAD-s28-atm-graft-timing-independent` | `../atm-core-worktrees/feature/pAD-s28-atm-graft-timing-independent` | the graft host-nudge readiness race is closed through deterministic readiness, not timeout luck |
| `AD.29` | `complete` | `feature/pAD-s29-phase-ad-post-send-smoke-matrix` | `../atm-core-worktrees/feature/pAD-s29-phase-ad-post-send-smoke-matrix` | one authoritative smoke lane proves the repaired post-send matrix |
| `AD.30` | `complete` | `feature/pAD-s30-windows-daemon-integration-depth` | `../atm-core-worktrees/feature/pAD-s30-windows-daemon-integration-depth` | Windows local-IPC depth coverage is restored, the direct-fix ledger is closed, and this readiness file records the final verdict |

## Direct-Fix Carry-Forward Ledger

These items were validated during earlier AD execution or phase-end review, but
their final closure evidence is owned by the phase-close artifacts rather than
by a new code sprint:

| Item | Technical owner | Closure-artifact owner | Required evidence |
| --- | --- | --- | --- |
| `AD9-BLANKPANE-001` | `AD.9` | `AD.30` | cite accepted-line evidence that closes `docs/plans/phase-AD/sprint-AD9.md` Acceptance Criteria `the validated-on-entry blank tmux_pane_id drift for team-lead and arch-ctm is repaired on the accepted baseline` |
| `ERRDOC-001` | `AD.9` | `AD.30` | cite accepted-line evidence that closes the `docs/plans/phase-AD/sprint-AD9.md` CLI Error Contract entries for `ATM_MEMBER_ALREADY_EXISTS`, `ATM_MEMBER_NOT_FOUND`, `ATM_IDENTITY_UNAVAILABLE`, `ATM_TEAM_UNAVAILABLE`, `ATM_IDENTITY_INVALID`, and `ATM_TEAM_INVALID` |
| historical `FTQ-001` record reconciliation | accepted-line code fix predates this follow-up | `AD.30` | either updated historical TTL closure status or an explicit note here explaining why the old discovery record remains open as provenance only |
| phase-AD triage sweep ledger | `AD.30` | `AD.30` | `.triage/phase-AD/direct-fix-track.md` populated with the final sweep disposition |
| `CHANGELOG.md` entry for `AD.13` through `AD.30` | `AD.30` | `AD.30` | release-facing changelog text present on the accepted line |

## Phase Exit Criteria

`AD.25` through `AD.30` follow-up closure is not complete until all of the following are
true:

- `AD.25` through `AD.30` all pass on the accepted line
- `docs/plans/phase-AD/readiness.md` exists and records the final verdict on
  the accepted line
- `.triage/phase-AD/direct-fix-track.md` exists and names the final owner plus
  closure artifact for the non-code obligations surfaced during plan review
- the AD18/ARCH-004 RULE-001 scope ruling is recorded on the accepted line
  through `docs/adr/ADR-020-rule001-observability-adapter-exception.md`:
  - `crates/atm-daemon/src/daemon_runtime_observability.rs` is the sanctioned
    library-internal adapter module allowed to import
    `sc_observability_types::{ActionName, OutcomeLabel}` directly
  - every other `crates/atm-daemon/src/` file routes those aliases through the
    sanctioned adapter module's crate-visible alias/helper surface
  - `.just/lint_boundaries.py` mechanically enforces that only the sanctioned
    adapter module remains allowlisted for this direct import pattern
- the accepted post-send line keeps `PostSendHookEmitter` attempt-only and
  `GraftPostSendPort` receiver-specific, with built-in override lookup upstream
  of the emitter after `AD.27`
- the authoritative post-send smoke lane from `AD.29` has recorded evidence
  for:
  - external hook success
  - external hook partial failure
  - built-in fallback
  - override reset-to-default
  - override disable behavior when retained
- the authoritative Windows daemon depth lane from `AD.30` has recorded
  evidence for:
  - dispatcher panic during shutdown
  - injected accept-error handling with one logged failure plus a bounded typed
    fail-fast exit
  - post-terminate connection rejection
- `CHANGELOG.md` contains the release-facing entry for the `AD.13` through
  `AD.30` corrective line
- the phase-close artifacts explicitly reconcile the historical `FTQ-001`
  discovery ledger with the accepted-line code fix so that historical
  provenance is not silently inconsistent with the current runtime
