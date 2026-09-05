# Issue #1171: Windows evidence (FastPC4)

## Provenance

- Candidate: `5875b70bdcc01337eac2a6fdeda1d1a48cc1d550`.
- Host/account: `FASTPC4`, `RZ\\rand.lee`.
- Benchmark profile root: `C:\\atm-bench\\home-1.5.0`; candidate runtime home:
  `C:\\atm-bench\\home-1.5.0\\.atm`.
- Host label: `windows-x64-01-isolated`.
- Daemon: one candidate release `target\\release\\atm-daemon.exe` per run,
  stopped and reaped after each run. Preflight found no ambient `atm-daemon.exe`.
- Scheduler: not used. The requested S4U/SYSTEM Scheduled Task could not be
  registered from this unelevated token (`Access is denied`); the operator
  explicitly authorized current-account execution. This is a provenance
  limitation, not a claim of noninteractive isolation.

## TCP f16/64

The documented wrapper was run first and failed before measurement. It starts
the daemon with the disposable `ATM_HOME`, but snapshots
`USERPROFILE\\.atm\\db`; the candidate daemon writes the durable database under
the former path, so the latter is absent. The retained failed summary is
`../send-message-benchmark/20260905-004651.854426-windows-x64-01-isolated-tcp-f16.json`.

The three retained direct TCP profiles use the candidate's unchanged
`start_capacity_daemon`, `direct_peer_endpoint`, and `run_profile` routines.
They omit only the wrapper snapshot/restore step above. Each interval sent
1,000 messages using f16/64; every request was accepted.

| Campaign | Daemon port | Samples | Accepted/requested | p50 msg/s | Gap vs. 8,793.91 | First-10 p50 | Last-10 p50 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 5960 | 134 | 134,000/134,000 | 6,562.88 | -25.37% | 8,237.35 | 6,347.11 |
| 2 | 27545 | 130 | 130,000/130,000 | 6,374.73 | -27.51% | 7,786.33 | 6,529.30 |
| 3 | 62587 | 127 | 127,000/127,000 | 6,257.15 | -28.85% | 7,661.60 | 6,149.16 |

Combined p50 is 6,392.87 msg/s, 27.30% below the retained 8,793.91 msg/s
floor. The harness-side timing captures end-to-end socket/client/daemon
completion per interval: 63 connections per 1,000-message interval, 20.0 s
per campaign, and no response or admission failures. It does not split CPU
time between client, daemon, and kernel. The first-to-last decile declines by
16.14% to 22.95% in every run, so this evidence points to sustained write-path
or SQLite-state growth under the candidate workload, not a fixed listener or
single-client connection failure. It does not isolate a specific candidate
commit and is not evidence for changing a floor.

## FTS fanout saturation

The rerun used the checked-in `query-fts` corpus, 32-way fanout, warmup, and
observation functions against one candidate daemon. The checked-in public read
runner currently rejects Windows before workload execution because it requires
POSIX identity capture; the retained helper invokes those unchanged workload
functions directly and documents that limitation.

- Workload: 32-way fanout, 2.0 s warmup, 5.0 s measurement; daemon port
  `25648`.
- Result: 3,573/3,680 successful, 107 failures (2.91%).
- Structured-log delta: 107 `ATM_DAEMON_CONNECTION_SATURATED` events.
- Doctor effective lanes: mailbox pool/depth `4/16`; search pool/depth `2/8`.
- Doctor exposes no saturation counter. The retained count is therefore the
  structured-log event count, not an invented runtime metric.

The failure strings say `bounded mailbox reader lane request failed` while the
workload is labelled `query-fts` and doctor reports the search lane. The
configured search capacity (two active readers plus depth eight) is below the
32-way workload and is consistent with saturation, but the lane-name mismatch
means this evidence does not prove which server lane emitted each rejection.

## Lint TypeError

`just lint` was run twice with the candidate's pinned Python `3.14.7`. The
final raw aggregate run passed all 35 checks; `pytests` ran 1,004 tests with 49
skipped. No `TypeError` or traceback reproduced, so no traceback can be
truthfully attached for this run.

Execution order:

`fmt, clippy, deny, shear, arch-gates, version, boundaries, adr-index,
unix-gating, same-host-portability, runtime-waits, manifests,
daemon-signing-coupling, silent-emit, function-length, legacy-mailbox-paths,
nudge-taxonomy, capability-degradation, identities, env-var-boundary,
runtime-observation-boundary, read-concurrency-gates, fixed-sleep, ttl-triage,
lines, spell, hermes-adapter, hermes-atm-boundary, atm-graft-python-boundary,
daemon-singleton, legacy-transport-removal, peer-dial-seam, sc-boundary,
sc-portability, pytests`.

## Reports index

`just reports-index --check` completed successfully after these artifacts were
written.

## Retained files

- `tcp-f16-64-campaign-1.log`: wrapper preflight failure and recovery attempt.
- `tcp-f16-64-current-account.json`: three direct TCP profiles and candidate
  doctor reports.
- `query-fts-32-way-current-account.json`: 32-way saturation reproduction,
  reader-lane report, failure samples, and structured-log count.
- `lint-current-account-raw.log`: aggregate lint output and all passing gate
  names.

No source, daemon-runtime, benchmark-floor, or baseline changes were made.
