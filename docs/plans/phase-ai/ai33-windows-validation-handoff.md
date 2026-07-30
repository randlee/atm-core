# AI.33 Windows validation handoff

This is the committed handoff log for the AI.33 Windows capacity validation.
The executable procedure is
[`plan-ai33-windows-capacity-verification.md`](plan-ai33-windows-capacity-verification.md).

## Working agreement

- cwin runs the procedure from this worktree on `fastpc4`.
- arch-ctm and cwin record a concise dated entry below for each handoff,
  result, or blocker.
- The writer commits and pushes the entry. Before writing, pull/rebase the
  branch; do not overwrite another entry.
- Link or name evidence artifacts, but do not commit generated databases,
  logs, certificates, or private runtime state.

## Entries

### 2026-07-30 — arch-ctm — handoff

- Windows worktree: `F:\\github\\atm-core-worktrees\\feature\\pAI-s33-admission-capacity-smoke`
- Baseline: `4038ca75` plus this handoff and the committed validation plan.
- Earlier `rand.lee` diagnostic is evidence only, not a valid acceptance run:
  `artifacts/smoke/admission-capacity-fastpc4/admission-capacity-20260730T030156Z.json`.
  It used the wrong OS account and must not be used to judge AI.33.
- cwin must follow the plan in order and commit this log with either the
  resulting evidence paths and summary, or the first deterministic blocker
  and its collected artifacts.

### 2026-07-30 — cwin — first-round root cause

- Pulled the branch; no newer arch-ctm direction was present in this shared file.
- `just test` passed. The live smoke/capacity acceptance gate remains blocked.
- First-round root-cause report: `artifacts/smoke/admission-capacity-fastpc4/windows-root-cause-report.md`.
- Missing `ATM_IDENTITY`/`ATM_TEAM`, missing `sc-compose`, and missing `atm` PATH resolution were environment/setup failures and were corrected operationally.
- The observed `127.0.0.1:55758` listener is the expected Windows local HTTP endpoint; 43101 is the separate peer HTTPS port. No daemon code failure was found.
- The report records all observed command errors, non-fatal test warnings, `.atm` log findings, orphan cleanup, and the next matched-pair smoke procedure.
