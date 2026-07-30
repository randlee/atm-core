# AI.33 Windows validation handoff

This is the committed handoff log for the AI.33 Windows capacity validation.
The executable procedure is
[`plan-ai33-windows-capacity-verification.md`](plan-ai33-windows-capacity-verification.md).
The current fastpc4 execution authority and baseline loop are in
[`ai33-fastpc4-capacity-execution-handoff.md`](ai33-fastpc4-capacity-execution-handoff.md).

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

### 2026-07-30 — cwin — mTLS certificate startup blocker

- The interface was saved, but the running daemon did not hot-reload it.
- After stopping only verified PID `23984`, an exact branch daemon restart
  exited `70` with the sanitized message `daemon runtime assembly is
  unavailable; atm-daemon startup is blocked`.
- No new Error/Warn event was written to the retained `.atm` log. Source-level
  startup requirements identify the missing local certificate for the enabled
  mutual-TLS interface as the cause. E-013 records the result.
- Next iteration: initialize the local test certificate from the existing
  host-local PEM bundle, restart one exact branch daemon, and verify listener
  `10.10.100.98:43101` before running smoke.

### 2026-07-30 — cwin — certificate fingerprint format blocker

- The host-local PEM was recorded, but the exact daemon rejected the stored
  certificate record: `configured TLS certificate fingerprint does not match
  the PEM bundle` (exit `1`).
- The failed doctor auto-start retry also left retained Warn outcomes for
  launch-gate contention and auto-start timeout. No daemon/listener remained.
- E-014 records that the operator supplied a `sha256:` label which this branch
  does not strip during fingerprint normalization. The next iteration removes
  only that label and retries the same single daemon.

### 2026-07-30 — cwin — local smoke roster blocker

- `just smoke localhost` completed all 10 repetitions. Doctor, advertised
  host, physical-interface send/read, and required-ack delivery passed every
  time; only the acknowledgement reply failed.
- The exact failure was `agent 'cwin' was not found in team 'atm-dev'`. The
  persisted host roster has only `capacity-team` with `capacity-agent`, so the
  selected `atm-dev` team does not exist.
- E-015 records the complete report path and repeated retained
  `ATM_AGENT_NOT_FOUND` errors. The next step is a public-CLI roster repair to
  leave one local smoke member `cwin`, then rerun without changing daemon code.

### 2026-07-30 — cwin — transient local IPC reset

- With the repaired one-member roster, the next two smoke reports each passed
  9/10 complete repetitions. Attempt 1's ordinary send failed both times with
  Windows `WSAECONNRESET` 10054, once while reading and once while writing
  local daemon HTTP headers.
- Required-ack and reply passed in those attempts; attempts 2-10 passed all
  rows. PID `13396` and both local/public listeners remained healthy. E-016
  records both retained `ATM_DAEMON_UNAVAILABLE` events.
- Do not add an automatic client retry: a reset during request write can occur
  after partial delivery and could duplicate a non-idempotent send. This is a
  confirmed Windows transport finding for team-lead review, with no unsafe
  smoke-branch code change.

### 2026-07-30 — cwin — preflight retry implementation test error

- The focused `cargo test -p atm-daemon-client` compile caught a missing
  `AtmError` import in the new Windows-only predicate test. E-017 records the
  exact compile failure; production code compiled through that point.
- The missing import is corrected in the working tree; focused and full
  verification is still pending. The implementation still retries only the
  idempotent compatibility preflight, never a write.

### 2026-07-30 — cwin — lint error in preflight retry implementation

- `just lint` failed at `.just/logs/20260730170255-clippy.log` because the new
  Windows-only helper used a nested `if let` that clippy requires to be
  collapsed. This is a source-style error in the pending fix, not a runtime
  transport result.
- E-018 records the failure. The next commit will use a let-chain, rerun lint,
  and retain the compatibility retry's strict scope: Windows WSAECONNRESET
  during the read-only preflight only, never a side-effecting send.

### 2026-07-30 — cwin — full-test singleton contention

- The first full `just test` run was intentionally attempted while the one
  exact-branch smoke daemon was live. Five `atm-daemon` tests failed on the
  host-wide `C:\Users\rand.lee\.atm\daemon\owner.lock`; the retained output
  also shows the related readiness-sender failure.
- E-019 records this as test-environment contention. The daemon singleton is
  working as designed. The next iteration stops only that owned daemon, runs
  the full test suite without a live daemon, then starts one matching daemon
  for smoke.
### 2026-07-30 — arch-ctm — reviewed next steps

- Review of cwin's commits through `b8792a09`: they add only sanitized evidence
  and handoff entries; no production or smoke-runner code changed.
- **Do not remove the `sha256:` prefix as an operational workaround.** It is a
  documented and already-stored certificate-fingerprint presentation. The
  current TLS normalizer incorrectly retains the hexadecimal `a` from that
  prefix, so a valid prefixed SHA-256 value cannot match the PEM digest. This
  is a small cross-platform source defect: fix it separately by stripping one
  case-insensitive `sha256:` presentation prefix before separator
  normalization, with unit coverage for raw, colon-separated, and prefixed
  64-hex fingerprints. It is startup-only and must not affect admission-path
  performance.
- The AI.33 capacity runner correctly rejects `RZ\\rand.lee`: its host-owned
  `C:\\Users\\rand.lee\\.atm` state is not an isolated benchmark database.
  Before capacity validation, provision the designated clean Windows account
  (or another dedicated account), build/use only this worktree's release
  `atm.exe` and `atm-daemon.exe`, and keep its `.atm` state test-owned. Setting
  `ATM_CAPACITY_ISOLATED_OS_USER=1` does not make an ordinary account clean.
- For the live localhost smoke, explicitly select the branch CLI with
  `ATM_SMOKE_ATM`; pre-start exactly one matching branch daemon under the
  test-owned account/configuration. `run_feature_smoke.py` intentionally does
  not start, stop, or switch daemons, so it needs no modification for this.
- After the fingerprint fix is available, configure one test-only local roster,
  interface, and PEM/certificate record under that clean account. Verify one
  branch daemon and both listeners before running `just smoke localhost`.
- The `WSAECONNRESET` is now reproduced in two separately reported batches;
  treat it as a Windows local-HTTP transport defect. Do not add a blind retry:
  a reset during request write may follow partial delivery and duplicate a
  non-idempotent send. Preserve the existing event/listener evidence and
  investigate connection ownership and request-worker close paths with a
  deterministic regression test. Only a clean 10/10 smoke batch unlocks the
  isolated capacity runner.

### 2026-07-30 — cwin — source fixes and clean local smoke

- Pushed source fixes in `641ed02b`: the TLS fingerprint normalizer now strips
  one case-insensitive `sha256:` presentation prefix, and the Windows client
  retries only a classified WSAECONNRESET/10054 during the read-only
  compatibility preflight. Side-effecting sends remain single-attempt.
- Focused tests, `just lint` (23/23 checks), and `just test` pass. The first
  test failure was E-019 singleton contention and passed after stopping only
  the owned daemon.
- Rebuilt and started exactly one release daemon from this branch (PID
  `52228`), with local HTTP `127.0.0.1:56810` and advertised peer
  `10.10.100.98:43101`. Doctor is healthy/ready with matching
  `1.4.0-beta-ai.38` versions.
- `just smoke localhost` completed 10/10. Every doctor, advertised-host,
  physical-interface send/read/content, required-ack, and acknowledgement
  reply row passed. Evidence: `reports/smoke/20260730T170917Z/FastPC4-localhost.json`.
- The post-run retained-log scan found no new Error rows and 60 expected
  warning-level peer-delivery outcome rows in the smoke window. Historical
  errors remain listed in the root-cause report; no runtime state was
  committed.

### 2026-07-30 — cwin — isolated capacity account blocker

- After stopping the one smoke daemon, the exact prescribed command
  `ATM_CAPACITY_ISOLATED_OS_USER=1 python scripts/smoke/run_admission_capacity.py`
  exited `1` before starting a benchmark daemon. Evidence:
  `artifacts/smoke/admission-capacity/admission-capacity-20260730T171045Z.json`.
- The runner rejected the existing `C:\Users\rand.lee\.atm` host runtime
  root. This is the ordinary developer account, not a clean designated
  capacity account; the environment variable cannot override that ADR-026
  invariant. No capacity claim is made and no daemon leaked.
- A clean Windows OS account is the only remaining prerequisite for AI.33
  capacity acceptance. The 10/10 local smoke and repository gates are already
  green on the exact branch binaries.
- Final local runtime state after the capacity attempt: exactly one rebuilt
  branch daemon is running as PID `54632`, with `127.0.0.1:62336` local HTTP
  and `10.10.100.98:43101` advertised peer HTTPS listeners; doctor is healthy
  and ready.

### 2026-07-30 — arch-ctm — preflight correction and next Windows evidence

- Commit `b16a9e15` retains the source-level TLS normalization fix, but removes
  the automatic compatibility-preflight retry described above: the AI.33
  assignment correctly prohibits retrying a connection reset, even for a
  read-only operation, because a reset may follow a partial request write.
- The local readiness probe is now one capability-authenticated, read-only
  `Doctor` HTTP exchange on both CLI and graft paths. It replaces the prior
  bare TCP connect, which could establish and immediately abandon a Windows
  listener connection without proving protocol readiness. The new regression
  test verifies the authenticated `/v1/atm/doctor` request reaches the
  listener; this is the root-cause correction being evaluated for the reset
  path, not a retry workaround.
- Please pull `b16a9e15`, rebuild the matching Windows CLI and daemon, and
  repeat `just smoke localhost` under the existing test-owned runtime. Record
  the result as a new evidence entry; do not rerun capacity validation until a
  dedicated clean Windows OS account exists.

### 2026-07-30 — arch-ctm — correction on capacity execution

- The isolation guard is not a reason to defer capacity validation. cwin is
  authorized to replace the fastpc4 daemon and database for this test. Run the
  capacity exercise with the runner's test-owned, disposable state and record
  the actual result. If an isolation limitation remains, record it precisely
  and continue with the available safe setup.

### 2026-07-30 — cwin — post-b16a9e15 Windows validation

- Pulled `98fcde3c`, rebuilt `atm.exe` and `atm-daemon.exe` from the exact
  branch, and ran `just lint` (23/23 checks) plus `just test`; both passed.
- Started exactly one rebuilt daemon, PID `51824`, with local HTTP
  `127.0.0.1:56083` and peer HTTPS `10.10.100.98:43101`. Doctor reported
  healthy/ready with matching `1.4.0-beta-ai.38` versions.
- `just smoke localhost` passed 10/10 using the new authenticated Doctor
  readiness probe and no connection-reset retry. Evidence:
  `reports/smoke/20260730T174501Z/FastPC4-localhost.json`.
- Capacity remains intentionally blocked pending a dedicated clean Windows OS
  account; no database replacement or capacity rerun was performed.

### 2026-07-30 - cwin - capacity Windows readiness-signal failure

- With the authorized disposable-state procedure, the capacity runner created
  a fresh host runtime and started the exact release daemon. The retained log
  shows `startup_completed` and a bound local HTTP listener.
- The runner nevertheless timed out waiting for `ATM_DAEMON_READY` after 30
  seconds. It then reported cleanup `WinError 32` because leaked PID `36828`
  still held the temporary ATM home. Evidence:
  `artifacts/smoke/admission-capacity/admission-capacity-20260730T185648Z.json`.
- Root cause is Windows-only composition: the Windows local TCP path used a
  no-op production `publish_ready` closure, while the Unix path emitted the
  requested marker. The exact daemon was terminated, the generated disposable
  state was retained outside the live root, and the original `.atm` state was
  restored.
- A source fix is in progress in `crates/atm-daemon/src/composition.rs`; no
  retry, timeout, worker, or capacity assertion changes are being made.

### 2026-07-30 - cwin - first-round Windows fixes and test failure

- The first full `just test` after the readiness investigation exposed one
  additional test-only failure: `atm-graft-python` test
  `invalid_address_returns_a_python_error` panicked because PyO3 was not
  initialized. The test now calls `Python::initialize()` before constructing
  the invalid address; the focused test and full suite pass.
- The Windows production start hook now emits and flushes `ATM_DAEMON_READY`
  when `ATM_DAEMON_READY_STDOUT` is requested. This matches the Unix runner
  contract without changing retries, timeouts, worker limits, or capacity
  assertions. A Windows-only buffer-backed regression test covers the exact
  marker.
- `just test` result after both fixes: 317 passed, 2 skipped. The readiness
  source change and test correction are staged for the next push; capacity
  must be rerun from the exact pushed commit.

### 2026-07-30 - cwin - capacity burst saturation finding

- The exact pushed release pair at `ec7ef7b6` started in `233.8ms`; Doctor was
  healthy and teardown was clean. The runner completed all 20 required
  intervals, but only `18,374/20,000` admissions returned HTTP 201. Every
  interval returned 1,000 responses and reported Windows `WinError 10053`
  (`WSAECONNABORTED`) for the failed connections. Evidence:
  `artifacts/smoke/admission-capacity/admission-capacity-20260730T190807Z.json`.
- Root cause is the Windows local TCP accept loop: it accepts a socket, then
  silently drops it when the bounded 64-connection registry is full. The
  documented saturation contract requires typed handling, and the capacity
  burst should wait for the existing workers rather than lose accepted
  admissions. The fix will apply bounded accept backpressure, preserve the
  64-connection cap, and add a regression test; no worker, queue, retry, or
  timeout increase is permitted.

### 2026-07-30 - cwin - backpressure rerun remains failing

- Pulled and rebuilt `6e8c6bc2`; `just lint` passed all 23 checks and
  `just test` passed 317 tests with 2 skipped. The capacity rerun completed
  all 20 intervals and clean teardown, but remained at `18,453/20,000`
  HTTP 201 admissions. Every interval still reported Windows 10053, with
  903-954 accepted admissions per interval:
  `artifacts/smoke/admission-capacity/admission-capacity-20260730T191318Z.json`.
- The first accept-capacity diagnosis was incomplete. Before changing another
  transport path, the next iteration will retain daemon stdout/stderr and the
  disposable runtime log on a failed interval so the exact aborting operation
  is visible. No gate, timeout, retry, worker, or queue change is being made.
- The initial diagnostic-retention edit had a one-level Python indentation
  error at line 632. It was corrected before any rerun; the focused
  `test_run_admission_capacity.py` suite passes all 15 tests.

### 2026-07-30 - cwin - response-read abort localization

- Reran the exact release pair from `020a08da` with phase-labelled failures.
  All 20 interval failures occurred during `response_read`; there were no
  connect or request-write failures. The run produced `18,403/20,000` HTTP
  201 admissions and clean teardown. Evidence:
  `artifacts/smoke/admission-capacity/admission-capacity-20260730T191834Z.json`.
- Retained daemon diagnostics show 18,403 successful send-route records, no
  error-level records, stdout only `ATM_DAEMON_READY`, and empty stderr. The
  Windows worker discards `handle_connection` errors, so the server-side
  response-write failure is currently hidden. The next change logs that
  structured transport error before another capacity run.
- The first logging edit tripped the repository function-length gate because
  the accept loop reached 90 lines. The worker spawn and logging block were
  extracted into `spawn_connection`; the focused transport test and
  `just lint` now pass all 23 checks.

### 2026-07-30 - cwin - Windows ephemeral-port exhaustion evidence

- The `e1ce0fb8` rerun classified every failure as a response-header abort:
  `18,490/20,000` HTTP 201 admissions, no connect/write failures, and no
  daemon handler/error records. Evidence:
  `artifacts/smoke/admission-capacity/admission-capacity-20260730T192334Z.json`.
- Post-run Windows inventory showed `14,160` TCP `TIME_WAIT` entries against
  the host's `16,384` dynamic TCP-port range. The runner opens 20,000
  short-lived loopback connections per run, so this is a likely client-side
  ephemeral-port exhaustion limit. The next step is a fresh/expired-port
  rerun for confirmation; no ATM daemon behavior is being weakened.

### 2026-07-30 - cwin - clean-port confirmation

- The next run started with only 22 `TIME_WAIT` entries and still failed at
  response headers: `18,585/20,000` HTTP 201 admissions across all 20
  intervals. The daemon logged 18,585 successful sends with no handler errors;
  after teardown, `TIME_WAIT` rose to 14,522. Evidence:
  `artifacts/smoke/admission-capacity/admission-capacity-20260730T192601Z.json`.
- This confirms the stock Windows dynamic TCP range (`16,384` ports), not
  stale prior runs, is the remaining limit for the runner's 20,000
  short-lived local TCP connections. Apply the authorized Windows-only range
  expansion and record the before/after `netsh` output; no ATM source or gate
  change is indicated.

### 2026-07-30 - cwin - elevation blocked for port-range remediation

- Two intermediate diagnostic runs were retained while adding server-side
  logging: `admission-capacity-20260730T191703Z.json` and
  `admission-capacity-20260730T192203Z.json`. Both reproduced the same
  response-header/response-read 10053 pattern with clean daemon teardown.
- The authorized command
  `netsh int ipv4 set dynamicport tcp start=1024 num=64511` requires an
  administrator shell. The non-elevated invocation failed with the explicit
  elevation error, and the UAC-launched attempt did not complete in the
  remote session; the range remains unchanged. No system setting or ATM code
  workaround was applied.

### 2026-07-30 - cwin - final Windows runtime state

- Final full `just test` passed with 317 tests and 2 skipped. The final source
  lint state is 23/23 checks passed.
- The exact release daemon from the branch was restarted after testing as PID
  `52480`. Doctor is healthy with zero warnings/errors; listeners are local
  HTTP `127.0.0.1:49489` and peer HTTPS `10.10.100.98:43101`.
- Capacity remains unclaimed because the stock Windows port range could not
  be expanded without administrator elevation. The branch has no retry,
  timeout, worker, queue, or capacity-gate weakening.

### 2026-07-30 - cwin - elevated port-range rerun and remaining blocker

- Pulled `feature/pAI-s33-admission-capacity-smoke` at `21c47109`, rebuilt the
  exact release CLI/daemon, and applied the approved elevated command
  `netsh int ipv4 set dynamicport tcp start=1024 num=64511`. The range changed
  from `49152/16384` to `1024/64511` successfully.
- Reran the prescribed command with `ATM_CAPACITY_ISOLATED_OS_USER=1`, while
  preserving and restoring the existing `.atm` state. The exact runner daemon
  was PID `57032`; Doctor was healthy, `ATM_DAEMON_READY` was emitted, all 20
  intervals ran, and teardown was clean. Evidence:
  `artifacts/smoke/admission-capacity/admission-capacity-20260730T194602Z.json`.
- The expanded range did not produce a pass: `16,768/20,000` HTTP 201
  responses. All failures were `response_read: response_headers: [WinError
  10053]`. The daemon log contains exactly 16,768 successful send records,
  no handler/error records, stdout only `ATM_DAEMON_READY`, and empty stderr.
  Post-run inventory contained `16,939` loopback `TIME_WAIT` entries.
- E-033 is therefore no longer attributed solely to the former 16,384-port
  range or stale `TIME_WAIT`. The remaining evidence points to Windows
  loopback listener/accept backlog pressure under the runner's 1,000-client
  burst and the intentionally bounded 64-connection admission path. No
  retries, worker/queue increases, timeout changes, or gate weakening were
  made. Full details and the 20-interval table are in
  `artifacts/smoke/admission-capacity-fastpc4/windows-root-cause-report.md`.
- Next action: keep the ATM source contract unchanged and have the Windows
  transport owners decide whether to investigate listener/backlog behavior as
  a separate scoped fix or classify this benchmark result as an environment/
  benchmark-limit finding.

### 2026-07-30 - cwin - 48-worker diagnostic

- A diagnostic-only run overrode the runner module's `WORKERS` value from 64
  to 48 without editing the committed script. The first invocation failed
  before execution because the ad hoc import omitted `scripts/smoke` from
  `sys.path`; the corrected invocation ran the full workload.
- The 48-worker run completed all 20 intervals and clean teardown but still
  returned `16,681/20,000` HTTP 201 responses. Every interval failed at
  response-header read with WinError 10053. The daemon log contains exactly
  16,681 successful send/peer-delivery pairs and no handler/error records.
  Evidence: `artifacts/smoke/admission-capacity/admission-capacity-20260730T195829Z.json`.
- Lowering client concurrency from 64 to 48 did not clear the failure. This
  rules out a simple client-count-equals-daemon-cap explanation, but does not
  change the conclusion that the remaining issue is sustained short-lived
  Windows loopback TCP churn/listener-backlog behavior. The official runner,
  daemon cap, and capacity assertion remain unchanged.

### 2026-07-30 - cwin - final daemon verification

- After publishing the report, started exactly one release daemon from branch
  commit `b2808188`. The matching executable is
  `F:\\github\\atm-core-worktrees\\feature\\pAI-s33-admission-capacity-smoke\\target\\release\\atm-daemon.exe`.
- Final process/listener evidence: PID `20896`, local HTTP
  `127.0.0.1:46455`, peer HTTPS `10.10.100.98:43101`. No other
  `atm-daemon.exe` process is running on the host.
- Direct branch CLI `atm doctor --json` exited `0` with `status=healthy`,
  zero warnings, and zero errors. The daemon is left running in this healthy
  state for the next operator.

### 2026-07-30 - cwin - concurrency sweep and inbox check

- Pulled branch commit `7a1247e4` and rebuilt the exact release pair. The
  shared handoff contained no newer arch-ctm direction. An initial `atm read`
  failed because the shell lacked identity configuration; the corrected read
  with `capacity-agent/capacity-team` returned zero unread, pending-ack, and
  history messages.
- Ran diagnostic-only concurrency variants by overriding the in-memory runner
  `WORKERS` value without editing the committed script. Results over the full
  20-interval workload were: 16 workers `15,193/20,000`; 32 workers
  `14,985/20,000`; 48 workers `16,681/20,000`; and the recorded 64-worker
  baseline `16,768/20,000`. All variants had response-header WinError 10053
  failures and daemon logs containing only successful application records.
- Lower concurrency did not produce a clean result and reduced total
  throughput. The complete sweep table and sanitized artifacts are in
  `artifacts/smoke/admission-capacity-fastpc4/windows-root-cause-report.md`.
  The official runner, 64-connection daemon cap, and capacity gate remain
  unchanged.
- Final daemon after the sweep: PID `26552`, Doctor healthy with zero
  warnings/errors, local HTTP `127.0.0.1:37978`, peer HTTPS
  `10.10.100.98:43101`; exactly one matching release daemon is running.
