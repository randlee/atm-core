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
- Source-fix commit: `641ed02b`
- Latest report commit before capacity attempt: `c14f983a`
- Latest pulled validation commit: `98fcde3c` (`b16a9e15` source correction)

## Result

`just lint`, `just test`, and the required 10/10 live localhost smoke passed on
the exact branch binaries. AI.33 capacity acceptance remains blocked by the
stock Windows dynamic TCP-port range: the authorized disposable-state run
reached all 20 intervals, but 20,000 one-connection-per-admission requests
cannot complete reliably within the 16,384-port range while closed sockets
remain in `TIME_WAIT`. The authorized range expansion requires administrator
elevation unavailable in this session.

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
| E-014 | Exact daemon restart after E-013 | `configured TLS certificate fingerprint does not match the PEM bundle` (exit `1`). A doctor auto-start retry also logged `daemon_launch_gate` contended and `daemon_auto_start` timeout_exhausted. | The stored `sha256:` label is valid certificate-fingerprint presentation, but the source normalizer treated the `a` in that prefix as hexadecimal input. The resulting value could not match the PEM digest. | Fixed in `641ed02b`: strip one case-insensitive `sha256:` presentation prefix before separator normalization, preserving raw and colon-separated forms. Unit coverage passes; the host record was not changed as a workaround. |
| E-015 | `just smoke localhost`, report `reports/smoke/20260730T165539Z/FastPC4-localhost.json` | All 10 attempts passed doctor, advertised-host, physical-interface send/read, and required-ack delivery. All 10 acknowledgement reply steps failed: `agent 'cwin' was not found in team 'atm-dev'`. | The smoke environment selected `ATM_TEAM=atm-dev`, but the host roster contains only `capacity-team` with `capacity-agent`; `atm-dev` is not persisted on this Windows account. Retained logs contain repeated `ATM_AGENT_NOT_FOUND` errors plus peer write/recovery warnings for the self target. | Pending operational fix: use the public roster commands to leave one local smoke team member `cwin`, set `ATM_TEAM` to that team, and rerun the same prescribed smoke. No production code change indicated. |
| E-016 | Second and third `just smoke localhost` runs, reports `reports/smoke/20260730T165659Z/FastPC4-localhost.json` and `reports/smoke/20260730T165810Z/FastPC4-localhost.json` | Attempt 1 ordinary physical-interface send failed in both runs with Windows `WSAECONNRESET` 10054: first while reading daemon HTTP headers, then while writing daemon HTTP request headers. The same attempts' required-ack and reply passed; attempts 2-10 passed all rows. | The retained events are `ATM_DAEMON_UNAVAILABLE` on the local loopback HTTP control path, not the 43101 peer listener. PID `13396` stayed alive, both listeners remained bound, and no daemon shutdown/error event occurred. The failure is reproducible at the first physical send after the runner's doctor call, with phase variation between write and read. | No safe retry was added: a reset during request write can occur after some bytes are accepted, so automatic retry of a non-idempotent send could duplicate a message. This requires a scoped Windows transport fix or an explicit team decision; no production change made in this smoke branch. |
| E-017 | Focused `cargo test -p atm-daemon-client` after the Windows preflight-retry change | Windows test compilation failed three times with `use of undeclared type AtmError` in the new predicate test. | The test module did not import the error type used by its new assertions. Production code compiled to this point; this was an implementation test-import omission. | Fixed in `641ed02b` by importing `atm_storage::AtmError`; focused tests and full `just test` pass. |
| E-018 | `just lint`, `.just/logs/20260730170255-clippy.log` | Clippy rejected the new Windows-only preflight helper: `this if statement can be collapsed` at `crates/atm-daemon-client/src/compatibility.rs:148-149`. | The initial implementation used nested `if let` and predicate checks. This is a source-style error in the new fix, not a platform behavior failure. | Fixed in `641ed02b` with a let-chain; `just lint` subsequently passed all 23 checks. |
| E-019 | First full `just test` with the smoke daemon running | Five `atm-daemon` tests failed with `DaemonServingStateRejected`: `a live ATM daemon already owns C:\Users\rand.lee\.atm\daemon\owner.lock`; one runtime-composition test also reported `daemon local ipc ready signal sender dropped before readiness`. | The full test suite uses the same user-scoped ATM home and host-wide daemon ownership lock as the live smoke daemon. Running both concurrently is intentionally rejected by the singleton contract; the readiness failure is the corresponding test startup consequence. This is test-environment contention, not a production regression. | Resolved as environment-only: stopped PID `13396`, reran `just test` with no daemon, and the full suite passed. The rebuilt daemon was then restarted once as PID `52228`. |
| E-020 | Retained `.atm` log scan after clean smoke report `reports/smoke/20260730T170917Z/FastPC4-localhost.json` | The log contains 17 historical `Error` rows from E-001/E-015/E-016, but no `Error` row during the clean 10/10 smoke window. It contains 60 `Warn` rows in that window: expected `peer_delivery` outcomes `write_persisted` and `peer_recovery_attempt` for the self-target physical-interface lane. | The warning-level peer-delivery records are the daemon's retained outcome telemetry for the prescribed self-target lane; they are not failed sends. The historical errors predate the clean run and remain documented in earlier rows. | No new runtime error. Evidence retained in the host log; no `.atm` state or log files committed. |
| E-021 | `ATM_CAPACITY_ISOLATED_OS_USER=1 python scripts/smoke/run_admission_capacity.py`, evidence `artifacts/smoke/admission-capacity/admission-capacity-20260730T171045Z.json` | Runner exited `1` before starting a daemon: `admission-capacity smoke requires a dedicated clean OS user whose host runtime root does not already exist: C:\Users\rand.lee\.atm`. | The current account is the normal developer account and already owns persistent ATM runtime/database state. The explicit environment variable acknowledges the required isolation; it cannot convert an existing account into a clean benchmark account. | Correctly blocked by the runner's ADR-026 guard. No Windows code fix is appropriate. Capacity remains unclaimed until a designated clean Windows OS account is provisioned; no daemon was leaked and the evidence artifact records zero benchmark runs. |
| E-022 | `ATM_CAPACITY_ISOLATED_OS_USER=1 python scripts/smoke/run_admission_capacity.py`, evidence `artifacts/smoke/admission-capacity/admission-capacity-20260730T185648Z.json` | The capacity daemon logged `startup_completed`, but the runner timed out with `capacity daemon did not publish ATM_DAEMON_READY within 30 seconds`. Cleanup then failed with Windows `WinError 32` because the temporary ATM home was still held by leaked exact-branch PID `36828`; the generated report recorded `doctor` failure and zero capacity intervals. | The Unix local IPC startup path emits `ATM_DAEMON_READY` when requested. The Windows local TCP production path passed `publish_ready: || Ok(())` from `RuntimeComposition::start`, so the runner waited for a marker that Windows never emitted. The orphaned process kept its pipe/file handles open, producing the secondary cleanup error. | Fix in progress: emit the marker from the Windows production start hook through a shared writer helper, with deterministic unit coverage. The failed disposable state is preserved outside the live ATM root at `C:\Users\rand.lee\.atm-ai33-capacity-failed-20260730-185648`; the original ATM state was restored. |
| E-023 | Full `just test` after the Windows readiness fix | The first full run failed only `tests::invalid_address_returns_a_python_error` in `atm-graft-python`: PyO3 reported `The Python interpreter is not initialized and the auto-initialize feature is not enabled` at `interpreter_lifecycle.rs:134:13`. | The test constructed a PyO3 value without initializing the interpreter, while the surrounding Python binding tests explicitly initialize it. This was a test-isolation defect exposed by the current PyO3 runtime contract, not a Windows production failure. | Fixed by calling `Python::initialize()` at the start of the test. The focused test passed and the subsequent full run passed: 317 tests, 2 skipped. |
| E-024 | Authorized disposable capacity run `20260730T190807Z`, evidence `artifacts/smoke/admission-capacity/admission-capacity-20260730T190807Z.json` | The daemon became ready in `233.8ms`, Doctor was healthy, and all 20 intervals returned 1,000 responses. Every interval still had `WinError 10053` (`WSAECONNABORTED`) failures, with only 872-943 HTTP 201 admissions per interval; total was 18,374/20,000. | Windows `serve_with_runtime_hooks` accepts a socket before reserving a registry slot. When `try_register(64)` returns `None`, the loop silently drops that already-accepted socket. The client observes 10053 instead of the documented typed saturation response, and the burst loses admissions even though 64 workers can drain the work. This is a Windows local TCP transport admission/backpressure defect, not a capacity assertion problem. | Fix in progress: stop calling `accept` while the bounded registry is full, wait for a connection slot to be released, and add regression coverage. This preserves the 64-connection cap and does not add workers, queues, retries, or timeout changes. |
| E-025 | Capacity rerun from `6e8c6bc2`, evidence `artifacts/smoke/admission-capacity/admission-capacity-20260730T191318Z.json` | The backpressure fix started the daemon and kept teardown clean, but all 20 intervals still reported `WinError 10053`; 18,453/20,000 admissions returned HTTP 201, with interval ranges of 903-954 accepted. | The first root cause was real but incomplete. The remaining abort occurs after the listener has capacity and requires retained daemon stdout/stderr and the capacity runtime log to distinguish handler/read/response failures from another Windows socket lifecycle issue. The current runner deletes the disposable runtime before those diagnostics are retained. | Not fixed. The next change is diagnostic-only: retain sanitized evidence paths for daemon stdout, stderr, and runtime logs when a capacity interval fails, without changing the capacity gate or retries. |
| E-026 | Focused `python -m unittest scripts/smoke/test_run_admission_capacity.py` after adding diagnostic retention | The runner could not import because of `IndentationError: unexpected indent` at `scripts/smoke/run_admission_capacity.py:632`. | The diagnostic patch inserted the cleanup-failure assignment one indentation level too deep. This was a local test-harness edit error, not a daemon/runtime failure. | Fixed immediately. The focused runner suite now passes all 15 tests; no capacity run used the malformed source. |
| E-027 | Capacity rerun from `020a08da`, evidence `artifacts/smoke/admission-capacity/admission-capacity-20260730T191834Z.json` plus retained diagnostics | All 20 interval failures were `response_read: [WinError 10053]`; no connect or request-write failures occurred. The daemon runtime log contains 18,403 `send/sent` records and no error-level records; stdout is only `ATM_DAEMON_READY` and stderr is empty. | The Windows worker currently evaluates `handle_connection(...)` as `let _ = ...`, so a response-header/body write error is discarded and never reaches the structured daemon log. The client sees the socket abort, but the server-side operation is not observable yet. | Fix in progress: log the structured `AtmError` from the Windows connection worker with transport context, then rerun. No client retry or capacity-gate change is being made. |
| E-028 | First `just lint` after adding Windows worker error logging, `.just/logs/20260730191930-function-length.log` | `function-length` failed with `RULE-002 failed: new hard violations ... local_tcp_transport.rs:233-322: serve_with_runtime_hooks (90 lines)`. | The new structured logging block overlapped a Windows accept-loop function that was already near the threshold; the prior backpressure block had made the function exceed the hard limit. | Fixed by extracting connection spawning and error logging into `spawn_connection`. The focused transport test and `just lint` now pass all 23 checks. |
| E-029 | Capacity rerun from `e1ce0fb8`, evidence `artifacts/smoke/admission-capacity/admission-capacity-20260730T192334Z.json` and post-run Windows socket inventory | All 20 interval failures were `response_read: response_headers: [WinError 10053]`; the daemon processed 18,490 sends with no handler/error records. After teardown the host had 14,160 TCP `TIME_WAIT` entries while Windows reported a 16,384-port dynamic TCP range. | The runner creates one new short-lived loopback TCP connection per admission and requires 20,000 connections in one run. Windows retains closed client ports in `TIME_WAIT`; once the ephemeral range is pressured, connections can establish but abort before response headers. This is client-side Windows resource exhaustion, not a daemon crash or response-handler error. | Pending disposition: verify on a fresh/expired `TIME_WAIT` pool before changing the runner. Do not add retries, increase daemon workers/queues/timeouts, or weaken the 1,000/second gate. |
| E-030 | Fresh-port rerun from `9642522d`, evidence `artifacts/smoke/admission-capacity/admission-capacity-20260730T192601Z.json` | The host started with only 22 `TIME_WAIT` entries, but the run still produced response-header 10053 failures in all 20 intervals: 18,585/20,000 HTTP 201 admissions. The daemon logged 18,585 successful sends and no handler errors; post-run `TIME_WAIT` was 14,522. | This confirms the stock Windows dynamic TCP range, not stale prior runs, is the limiting condition: 20,000 one-connection-per-admission requests exceed the 16,384 available dynamic TCP ports while closed sockets remain in `TIME_WAIT`. | Windows-only environment fix authorized: expand the dynamic IPv4 TCP range before the next run and record the prior/new values. No ATM source, retry, timeout, worker, queue, or gate change is required for this host limitation. |
| E-031 | Intermediate diagnostic runs from `4330b344` and `b1fa33c0`, evidence `admission-capacity-20260730T191703Z.json` and `admission-capacity-20260730T192203Z.json` | Both additional 20-interval runs completed daemon startup and clean teardown but reproduced response-header/response-read 10053 failures with no daemon stderr or handler-error records. | These runs are consistent with E-029/E-030 and were retained while adding phase labels and structured server-side error logging; neither revealed a distinct ATM code failure. | Covered by the committed diagnostic and logging changes; no separate production fix indicated. |
| E-032 | `netsh int ipv4 set dynamicport tcp start=1024 num=64511` | Non-elevated shell returned `The requested operation requires elevation (Run as administrator)`. Before and after remained `Start Port: 49152`, `Number of Ports: 16384`. The UAC-launched attempt did not complete in the remote shell. | The current Windows session cannot change the global TCP dynamic-port range without administrator elevation. | No system setting was changed and no unsafe socket workaround was added. Capacity remains blocked on this host until an administrator applies the documented range change or supplies a clean capacity host with sufficient ephemeral ports. |

## Key Contract Clarification

## Final Validation State

- Branch tip: `36139c9e`.
- `just lint`: passed all 23 checks after the final source change
  (`b1fa33c0` plus diagnostic runner `e1ce0fb8`).
- `just test`: passed with 317 tests and 2 skipped on the final pushed tree.
- Latest capacity evidence: `admission-capacity-20260730T192601Z.json`,
  `18,585/20,000` accepted with response-header 10053 failures; this is not
  claimed as a capacity pass.
- Windows port range remained unchanged at `49152-65535` because the
  administrator-only `netsh` update could not be completed.
- Final exact release daemon: PID `52480`; Doctor healthy with zero warnings
  or errors; local HTTP `127.0.0.1:49489`; peer HTTPS
  `10.10.100.98:43101`.

## Capacity Interval Evidence

The exact pushed release pair (`ec7ef7b6`) completed startup, Doctor, peer
configuration, and teardown. The runner's 20 required intervals produced the
following sanitized evidence. Each row has 1,000 responses; `accepted` counts
HTTP 201 responses.

| Case | Interval | Accepted | Elapsed (s) | Responses | First failure |
| --- | ---: | ---: | ---: | ---: | --- |
| accepting | 1 | 915 | 0.549 | 1000 | WinError 10053 |
| accepting | 2 | 908 | 0.535 | 1000 | WinError 10053 |
| accepting | 3 | 937 | 0.566 | 1000 | WinError 10053 |
| accepting | 4 | 872 | 0.509 | 1000 | WinError 10053 |
| accepting | 5 | 917 | 0.536 | 1000 | WinError 10053 |
| accepting | 6 | 895 | 0.528 | 1000 | WinError 10053 |
| accepting | 7 | 921 | 0.603 | 1000 | WinError 10053 |
| accepting | 8 | 938 | 0.501 | 1000 | WinError 10053 |
| accepting | 9 | 929 | 0.535 | 1000 | WinError 10053 |
| accepting | 10 | 928 | 0.534 | 1000 | WinError 10053 |
| unavailable | 1 | 905 | 0.564 | 1000 | WinError 10053 |
| unavailable | 2 | 912 | 0.588 | 1000 | WinError 10053 |
| unavailable | 3 | 929 | 0.547 | 1000 | WinError 10053 |
| unavailable | 4 | 916 | 0.546 | 1000 | WinError 10053 |
| unavailable | 5 | 922 | 0.600 | 1000 | WinError 10053 |
| unavailable | 6 | 924 | 0.589 | 1000 | WinError 10053 |
| unavailable | 7 | 925 | 0.516 | 1000 | WinError 10053 |
| unavailable | 8 | 943 | 0.527 | 1000 | WinError 10053 |
| unavailable | 9 | 897 | 0.526 | 1000 | WinError 10053 |
| unavailable | 10 | 941 | 0.543 | 1000 | WinError 10053 |

Aggregate: `18,374/20,000` accepted; slowest runner stage was
`unavailable_burst_ms` at `5562.0ms`. The daemon process was PID `59148`, and
the runner terminated it cleanly; no exact-branch daemon or listener remained.

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

## E-016 Follow-Up

The later correction in `b16a9e15` removes the preflight retry entirely and
uses one capability-authenticated Doctor exchange for readiness. It deliberately
does not retry the side-effecting send that produced E-016. The clean 10/10 run
had no reset, but does not prove the daemon connection-ownership defect is fully
root-caused. Team follow-up remains required for a deterministic transport
regression test.

## Final Runtime State

After the capacity attempt, exactly one rebuilt branch daemon is running as
PID `54632`, with local HTTP `127.0.0.1:62336` and advertised peer HTTPS
`10.10.100.98:43101` listeners. Doctor reports healthy and ready.

## Latest Reset Correction Validation

After pulling `98fcde3c`, rebuilding the release CLI and daemon, and running
the full repository gates with no daemon active, the updated readiness path was
validated with `just smoke localhost`. All 10/10 attempts passed Doctor,
advertised-host, physical-interface send/read/content, required-ack delivery,
and acknowledgement reply. Evidence:
`reports/smoke/20260730T174501Z/FastPC4-localhost.json`.

The updated source uses one capability-authenticated Doctor HTTP exchange for
local readiness and does not retry connection resets. The final exact-branch
daemon is PID `51824`, with local HTTP `127.0.0.1:56083` and peer HTTPS
`10.10.100.98:43101` listeners.
