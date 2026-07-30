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
