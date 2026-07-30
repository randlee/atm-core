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

### 2026-07-30 — cwin — local CLI/curl iteration

- Pulled the branch; no newer arch-ctm direction was present.
- Reused exactly one exact-branch release daemon, PID `23984`, and verified
  healthy/ready doctor status with matching CLI and daemon versions.
- Corrected local CLI doctor and curl doctor both passed. The first curl probe
  used malformed PowerShell JSON quoting and is logged as E-011 in the report.
- The next blocker is configuration: `atm peer interface list --json` is empty,
  so the advertised-local-IP lane cannot start. No second daemon was started.
- Full details are in
  `artifacts/smoke/admission-capacity-fastpc4/windows-root-cause-report.md`.

### 2026-07-30 — cwin — peer-interface configuration blocker

- Pulled again; no newer arch-ctm direction was present.
- The exact branch CLI returned `[]` from `atm peer interface list --json`.
- Host address discovery found `10.10.100.98` on `Ethernet 2`; no Windows
  `atm-daemon` service is installed, and the one exact branch daemon remains
  PID `23984`.
- E-012 is logged in the root-cause report. The planned operational fix is one
  enabled `10.10.100.98:43101` interface, followed by a controlled restart of
  only the verified branch daemon if configuration is not hot-reloaded.
