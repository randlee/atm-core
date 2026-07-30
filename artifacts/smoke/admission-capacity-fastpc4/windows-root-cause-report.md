# AI.33 Windows Root-Cause Report

## Scope

- Worktree: `F:\github\atm-core-worktrees\feature\pAI-s33-admission-capacity-smoke`
- Branch: `feature/pAI-s33-admission-capacity-smoke`
- Source commit at first-round investigation: `e057c39adb6e599de552aabb692481366b9a5bee`
- Host account: `RZ\rand.lee`
- ATM version: `1.4.0-beta-ai.38`
- atm-daemon version: `1.4.0-beta-ai.38`
- just: `1.51.0`
- sc-compose: `1.2.0`, installed from sibling checkout `F:\github\sc-compose`
- First-round report commit: pending initial push, recorded in the next log update

## Result

`just test` passed. AI.33 acceptance is not complete because the required live
smoke was not executed with a pre-started matched daemon pair, and the capacity
step was not run. No production-code fix was justified by the first-round
evidence.

## Error Log

| ID | Source | Observed error or warning | Root cause | Status / fix |
| --- | --- | --- | --- | --- |
| E-001 | `just smoke localhost`, first invocation | `set ATM_IDENTITY and ATM_TEAM before running live daemon smoke` | Required live-smoke identity context was absent from the shell. | Resolved for diagnostics with `ATM_IDENTITY=cwin` and `ATM_TEAM=atm-dev`; not a code defect. |
| E-002 | `just smoke localhost`, first configured invocation | Ten doctor rows reported `[WinError 2] The system cannot find the file specified`; report rendering also failed with `sc-compose render failed: [WinError 2]`. | `atm` was not on `PATH`, and the report renderer `sc-compose` was not installed. | Resolved operationally: cloned `randlee/sc-compose` and installed v1.2.0; branch release CLI path must be supplied explicitly. |
| E-003 | `atm-daemon.exe --version` probe | `unknown atm-daemon argument: --version` | The daemon executable does not expose a `--version` CLI option. | Recorded daemon package version from Cargo metadata instead. |
| E-004 | `just smoke localhost` after installing sc-compose | Ten doctor rows reported `[WinError 2]`; generated report `20260730T034550Z`. | The runner still invoked `atm` by name while `atm.exe` was absent from `PATH`; `ATM_SMOKE_ATM` is the runner CLI-path override. | Resolved by using the exact release `atm.exe` path; `ATM_DAEMON_BIN` should also identify the paired daemon when switching binaries. |
| E-005 | Full-path smoke retry | Runner exceeded its bounded execution window without a final report; runner-owned PID `33936` remained. It listened on `127.0.0.1:55758`, not 43101/43102. | `55758` is the expected Windows local HTTP endpoint because production code binds `127.0.0.1:0` and publishes the assigned port in `local-http.json`. 43101 is the peer HTTPS listener, not same-host CLI IPC. The smoke runner is documented not to start or manage a daemon; relying on CLI auto-start was the wrong setup. | Diagnostic daemon terminated. No listener or daemon remains. No production code change made. |
| E-006 | Prior untracked capacity artifact `admission-capacity-20260730T030156Z.json` | `capacity daemon did not publish ATM_DAEMON_READY within 30 seconds`; `sc-compose render failed: [WinError 2]`; leaked PID `29872`; no intervals. | Earlier diagnostic used the wrong OS account and lacked the required composition dependency, as noted by arch-ctm. | Retained as invalid historical evidence; excluded from AI.33 acceptance. |
| E-007 | `just test` output | `cargo-deny DNS resolution failed; retrying (1/3)` and `README.md: reviewed_for_release is 0.0.0, expected 1.3.0`. | Transient dependency DNS retry and existing release-document metadata warning. | Recipe exited 0; no test failure. |
| E-008 | `just test` embedded smoke output | Two fixture-style `FAIL` lines appeared for an acknowledgement reply and an IPv6 advertised-IP case. | These are fixture scenario output inside the passing repository test run, not failed test assertions. | `just test` exited 0; retained as non-fatal output. |
| E-009 | `.atm/logs/atm.log.jsonl` | No Error/Warn records or error outcomes were found. Daemon entries were Info-level `start_requested`, `install_hooks`, `acquire_owner_lock`, `bind_listener`, and `startup_completed`. | No daemon-side error was logged. | Direct release `atm doctor --json` subsequently returned healthy/ready with matching CLI and daemon versions. |
| E-010 | Diagnostic cleanup | Orphaned exact-branch daemon PIDs `33936`, `54896`, and `59404` were found during separate diagnostics. | The CLI auto-start child can outlive an externally terminated smoke wrapper; this is a test execution cleanup issue, not evidence of a daemon crash. | Each was terminated only after ownership/path verification. Current daemon count is zero. |

## Key Contract Clarification

`ATM_SMOKE_ATM` is read only by `scripts/smoke/run_feature_smoke.py` and
selects the `atm` CLI command. It does not set `ATM_DAEMON_BIN`, configure a
listener, or change the transport. The matched branch pair must be managed
before smoke execution, or both paths must be explicit:

```powershell
$env:ATM_SMOKE_ATM = "F:\github\atm-core-worktrees\feature\pAI-s33-admission-capacity-smoke\target\release\atm.exe"
$env:ATM_DAEMON_BIN = "F:\github\atm-core-worktrees\feature\pAI-s33-admission-capacity-smoke\target\release\atm-daemon.exe"
```

The Windows local transport intentionally binds an ephemeral loopback port and
publishes it in `C:\Users\rand.lee\.atm\daemon\local-http.json`. The public
peer interface, when configured, is a separate 43101 listener.

## First-Round Actions

- Pulled `feature/pAI-s33-admission-capacity-smoke`; no arch-ctm update was present in the shared handoff.
- Installed `sc-compose` v1.2.0 from source using Cargo.
- Rebuilt `atm.exe` and `atm-daemon.exe` from the exact branch release profile.
- Verified owner-only Windows ACL on `local-http.json`.
- Verified direct `atm doctor --json` returns healthy/ready with matching versions.
- Terminated only verified orphaned branch daemons.
- Did not run the capacity command because the plan requires stopping at the first live-smoke failure.

## Next Iteration

Before the next smoke attempt, pull the branch and re-read the shared handoff.
Use the daemon-switch procedure to start exactly one matched release pair,
verify doctor readiness, then run `just smoke localhost`. Commit and push this
report after each update.
