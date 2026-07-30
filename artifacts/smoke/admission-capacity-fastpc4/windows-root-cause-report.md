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
- First-round report commit: `eec67b217d7a0f224475a61f01adc6ebeac28db8`

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
| E-011 | Direct local curl probe | First request returned HTTP 400: `invalid doctor HTTP request body: key must be a string at line 1 column 2`. | PowerShell single-quoted JSON contained literal backslashes (`{\"...`) instead of valid JSON; this was a probe-command quoting error, not an ATM failure. | Corrected the request with `ConvertTo-Json -Compress`; curl exited 0 and returned healthy/ready doctor JSON from the exact branch daemon. |
| E-012 | `atm peer interface list --json` | The exact branch CLI returned an empty list (`[]`); no enabled advertised interface was available for the physical-interface smoke lane. | This Windows host had no persisted peer interface in its user ATM state. The host's usable IPv4 address is `10.10.100.98` on `Ethernet 2`; this is a setup/configuration gap, not a transport failure. | Pending immediate operational fix: save one enabled interface bound to `10.10.100.98:43101` and advertise `10.10.100.98`, then verify the single daemon reloads it. |
| E-013 | Exact daemon restart after E-012 | `atm-daemon.exe` exited immediately with code `70`: `daemon runtime assembly is unavailable; atm-daemon startup is blocked`. | The enabled mutual-TLS interface had no local certificate configured. The daemon startup composition requires a local certificate before binding any enabled mTLS HTTPS interface. The retained `.atm` log added no Error/Warn record; this is the daemon's sanitized startup error contract. | Pending operational fix: initialize the local test certificate from the existing host-local PEM bundle, restart one exact branch daemon, and verify the 43101 listener. No production code change indicated. |
| E-014 | Exact daemon restart after E-013 | `configured TLS certificate fingerprint does not match the PEM bundle` (exit `1`). A doctor auto-start retry also logged `daemon_launch_gate` contended and `daemon_auto_start` timeout_exhausted. | The certificate record was initialized with a `sha256:` label. The branch daemon normalizes only hexadecimal/colon fingerprint material, so the label changed the compared value and could not match the PEM certificate. | Pending operational fix: replace the record with the same fingerprint bytes without the `sha256:` label, then restart one exact branch daemon. No production code change indicated. |
| E-015 | `just smoke localhost`, report `reports/smoke/20260730T165539Z/FastPC4-localhost.json` | All 10 attempts passed doctor, advertised-host, physical-interface send/read, and required-ack delivery. All 10 acknowledgement reply steps failed: `agent 'cwin' was not found in team 'atm-dev'`. | The smoke environment selected `ATM_TEAM=atm-dev`, but the host roster contains only `capacity-team` with `capacity-agent`; `atm-dev` is not persisted on this Windows account. Retained logs contain repeated `ATM_AGENT_NOT_FOUND` errors plus peer write/recovery warnings for the self target. | Pending operational fix: use the public roster commands to leave one local smoke team member `cwin`, set `ATM_TEAM` to that team, and rerun the same prescribed smoke. No production code change indicated. |
| E-016 | Second and third `just smoke localhost` runs, reports `reports/smoke/20260730T165659Z/FastPC4-localhost.json` and `reports/smoke/20260730T165810Z/FastPC4-localhost.json` | Attempt 1 ordinary physical-interface send failed in both runs with Windows `WSAECONNRESET` 10054: first while reading daemon HTTP headers, then while writing daemon HTTP request headers. The same attempts' required-ack and reply passed; attempts 2-10 passed all rows. | The retained events are `ATM_DAEMON_UNAVAILABLE` on the local loopback HTTP control path, not the 43101 peer listener. PID `13396` stayed alive, both listeners remained bound, and no daemon shutdown/error event occurred. The failure is reproducible at the first physical send after the runner's doctor call, with phase variation between write and read. | No safe retry was added: a reset during request write can occur after some bytes are accepted, so automatic retry of a non-idempotent send could duplicate a message. This requires a scoped Windows transport fix or an explicit team decision; no production change made in this smoke branch. |
| E-017 | Focused `cargo test -p atm-daemon-client` after the Windows preflight-retry change | Windows test compilation failed three times with `use of undeclared type AtmError` in the new predicate test. | The test module did not import the error type used by its new assertions. Production code compiled to this point; this was an implementation test-import omission. | Pending immediate test-only correction: import `atm_storage::AtmError`, rerun the focused test, then run full lint/test. |

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

## Local CLI/Curl Iteration

- Pulled the branch again before this iteration; no newer arch-ctm direction was
  present in the shared handoff.
- Reused exactly one already-running exact-branch release daemon, PID `23984`,
  with local endpoint `127.0.0.1:62527`; doctor reported healthy/ready and
  matching `1.4.0-beta-ai.38` CLI/daemon versions.
- The corrected local branch CLI doctor command passed.
- The corrected curl doctor request passed. The failed first curl request is
  retained as E-011 so the malformed-payload root cause is not lost.
- `atm peer interface list --json` returned `[]`; local CLI/curl health is
  proven, but the advertised-local-IP smoke lane cannot proceed until the
  Windows peer interface is configured. No second daemon was started.

## Peer Listener Restart Iteration

- Saved the one enabled interface with bind `10.10.100.98:43101` and advertise
  host `10.10.100.98` using the branch CLI.
- The live daemon did not hot-reload the interface. After verifying PID `23984`
  was the only exact branch daemon, it was stopped and the exact branch daemon
  was restarted once.
- The restart failed before serving because no local mTLS certificate was
  configured. E-013 records the exact sanitized error and exit code. The
  stale loopback runtime file was not treated as proof of a live daemon.

## Certificate Fingerprint Iteration

- Initialized the host-local PEM bundle as the local certificate, then started
  one exact branch daemon attempt. It exited before serving.
- Direct execution exposed the exact error: `configured TLS certificate
  fingerprint does not match the PEM bundle` (exit `1`).
- The subsequent doctor auto-start retry generated retained Warn outcomes for
  `daemon_launch_gate` contention and `daemon_auto_start` timeout exhaustion;
  no daemon remained running and no 43101 listener was present.
- E-014 records the operator-input root cause: the stored `sha256:` label is
  not accepted by the branch fingerprint normalization. The next fix removes
  that label while retaining the same certificate bytes.

## Local Smoke Roster Iteration

- `just smoke localhost` ran all 10 required live repetitions. Doctor,
  advertised-host, physical-interface send/read, and required-ack delivery
  passed in all 10 attempts.
- The acknowledgement reply failed in all 10 attempts because `cwin` was not
  present in the selected `atm-dev` team. The complete JSON report is
  `reports/smoke/20260730T165539Z/FastPC4-localhost.json`.
- `atm teams --json` showed only `capacity-team` with one existing member,
  `capacity-agent`; `atm members --team atm-dev --json` confirmed the selected
  team was absent. E-015 records the repeated `ATM_AGENT_NOT_FOUND` errors and
  the associated retained peer delivery/recovery warnings.
- The next repair is host-state only: make one local team contain `cwin`, then
  rerun with that exact team context. No daemon restart or code change is
  indicated by this failure.

## Local IPC Reset Iteration

- After the roster repair, the second smoke report completed all 10 attempts.
  Nine attempts passed every row. Attempt 1 passed doctor, advertised host,
  required-ack delivery, and acknowledgement reply, but its ordinary send
  failed on the local daemon HTTP path with Windows `WSAECONNRESET` 10054.
- The retained log contains one corresponding `ATM_DAEMON_UNAVAILABLE`
  `service` event at `2026-07-30T16:56:55.581071Z`. PID `13396` remained alive,
  `127.0.0.1:57120` and `10.10.100.98:43101` stayed bound, and later requests
  succeeded. No crash or listener loss was observed.
- E-016 reproduced in the next run. In both runs, attempt 1's ordinary
  physical-interface send reset on the Windows local HTTP connection, while
  the same attempt's required-ack send/reply and attempts 2-10 passed. The
  reset occurred during response-header read in `20260730T165659Z` and request
  header write in `20260730T165810Z`.
- The daemon remained alive with both listeners bound throughout. No safe
  client retry was added because a reset during a request write does not prove
  that zero request bytes reached the daemon; retrying `send` could duplicate
  a non-idempotent write. This is now a confirmed Windows transport finding,
  not an unexplained one-off.

## Next Iteration

Before the next smoke attempt, pull the branch and re-read the shared handoff.
Use the daemon-switch procedure to start exactly one matched release pair,
verify doctor readiness, then run `just smoke localhost`. Commit and push this
report after each update.
