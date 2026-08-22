# AL.9 benchmark gate

**Status:** contract and immutable baseline frozen. One current-runtime local
TCP/f64 row is recorded below; the required multi-platform, transport, and
hook-mode matrix remains pending. This is deliberately a gate, not a
performance-pass claim.

## Baseline identity

The AL baseline is the committed `develop` revision
`67401907039f92e58e883273f02372a637202f70`, captured before AL.1 added the
Tokio/Axum dependency graph. The following compact artifacts are present in
that exact Git tree and are the local macOS comparison workload:

| Transport | Frames/connection | Baseline artifact | Source revision in artifact | Throughput p50 (/s) | Status |
| --- | ---: | --- | --- | ---: | --- |
| UDS | 1 | `site/reports/send-message-benchmark/20260801-072313.590684-mac-arm64-01-uds-f1.json` | `fb6b26363c5bb0bbb11a6cf167d090385dc53d35` | 180.94 | **Invalid**: `passed=false`; only 25/1,000 messages were accepted before the SQLite connection-budget error. It is not a baseline. |
| UDS | 2 | `site/reports/send-message-benchmark/20260801-072502.577968-mac-arm64-01-uds-f2.json` | `3ec7ce1ff7269d8f43a65658c712778abbf2de14` | 22,835.66 | Passing historical artifact. |
| TCP | 1 | `site/reports/send-message-benchmark/20260801-072723.571920-mac-arm64-01-tcp-f1.json` | `3ec7ce1ff7269d8f43a65658c712778abbf2de14` | 11,847.59 | Passing historical artifact. |
| TCP | 2 | `site/reports/send-message-benchmark/20260801-072744.295213-mac-arm64-01-tcp-f2.json` | `3ec7ce1ff7269d8f43a65658c712778abbf2de14` | 19,674.03 | Passing historical artifact. |

The embedded source revisions identify the binaries that produced historical
data; their presence in the pinned `67401907` tree makes the passing rows
historical context. The UDS/f1 row is explicitly retracted: its embedded
result has been failed since its first commit and cannot support a comparison.
No genuinely passing pre-AL UDS/f1 artifact has been retained, so the AL.9
gate has no valid UDS/f1 regression baseline. They do **not** make any row a
result for `atm-http-runtime`.

## Fixed current-runtime workload and pass rule

The runner is `scripts/smoke/run_admission_capacity.py` at the AL.9 proof
revision. For each platform/transport profile it must use:

- release-built `atm-daemon`, which selects
  `atm_daemon_bootstrap::run_replacement_daemon` and `atm-http-runtime`;
- the public authenticated `POST /v1/atm/messages` route with unchanged
  `WriteRequest` JSON and response handling;
- 64 workers, ten independent 1,000-admission intervals, a minimum 20-second
  duration, and one- and two-frame profiles (plus the existing sparse profiles
  only as supplemental data);
- durable SQLite verification, complete response consumption, raw interval
  samples, host label, OS, CPU architecture, `rustc -Vv`, release-binary path,
  and proof SHA;
- p50 and p99 end-to-end client latency, throughput p50, and error count;
- one hook-disabled and one hook-active run using the same request workload.

The acceptance rule is: no error or lost durable admission, at least 1,000
admissions/s in every interval, and no more than a 10% regression in throughput
p50 or more than a 20% regression in p99 latency from the matching
baseline-platform/transport/profile. A different host is reported, never
combined with this comparison. Windows must run TCP physically; it cannot be
substituted with an equivalent host.

## Current measurement state

The following compact artifact is a real current-runtime
`atm-http-runtime` measurement, committed in `1e82cd3c`:

| Host | Transport | Frames/connection | Artifact | Source revision | Throughput p50 (/s) | Durable restart |
| --- | --- | ---: | --- | --- | ---: | --- |
| `local` | TCP | 64 | `site/reports/send-message-benchmark/20260809-193238.569731-local-tcp-f64.json` | `11a6d52cf0304b4c61f3bb0770787453189a5908` | 6,660.38 | pass (133,000/133,000) |

This row clears the minimum 1,000 admissions/s floor for its own isolated
local TCP/f64 run. It does **not** close the gate: it has no matching frozen
baseline comparison, no separately recorded hook-active companion, and is not
a physical M5 or Windows result. The runner correctly refuses to attach to or
replace an ambient daemon and requires an isolated OS user.

Historical note: this AL.9 harness design was superseded by AO.4 because an
alternate benchmark executable cannot prove the shipped daemon's performance.
Current public commands are `just benchmark --target tcp` and
`just benchmark --target tcp-tls`; each launches the shipped Tokio/Axum
`atm-daemon` with its explicit peer-wire mode and retains the active hook.
The historical rows above are not AO.4-compatible baseline evidence without
new shipped-daemon provenance. Current-runtime rows not enumerated above,
including hook-active and Windows, remain **pending** until an authorized
operator executes the isolated benchmark gate.

### Retired managed-host backup/restore mode

`ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE` and the managed-daemon benchmark
arguments are retired. Moving, replacing, or restoring `~/.atm/db` is not a
durable backup protocol and may destroy the active OS user's data. The runner
fails before it can call `daemon-switch`, quiesce an ambient daemon, or mutate
that database. Every physical benchmark must run under a dedicated clean OS
user with `ATM_CAPACITY_ISOLATED_OS_USER=1`. A durable backup/recovery design
is planned separately and is not an authorization to benchmark against the
live database.

## Required artifacts before closure

1. Retain raw runner output outside `site/`, then commit the compact schema
   summary for each current-runtime profile.
2. Record both hook modes, the exact baseline artifact used for comparison,
   actual hardware/OS/toolchain, p50/p99/throughput/error values, and the
   tolerance calculation.
3. Retain an actual Windows TCP result at the same proof revision.
4. If any required comparison fails, record failure, park AL, keep the legacy
   activation state unchanged, and do not freeze AM's ledger.
5. Before measuring, build the feature-gated benchmark binary above; retain
   both explicit mode values in the raw and compact schemas. It uses the
   existing `MessageReceivedHookSelector` injection boundary and adds no
   daemon config fallback, sender hook, or second request path.

## Final pre-merge evidence pass

Do not backfill these items during ordinary branch review. Immediately before
the `integrate/phase-al` to `develop` decision, run one coordinated final
Tokio/Axum evidence pass and add only its resulting report links here:

1. Capture current-runtime M5 and Windows TCP benchmark rows at the final
   candidate SHA, including the required durable-count, latency, hook-mode,
   and raw-sample fields. Windows must remain a physical Windows TCP run.
2. Capture a real graft write and its CLI activation/read path using the final
   candidate; source-selection or historical graft artifacts are not runtime
   proof.
3. Capture the cutover invariant at runtime: exactly one active
   `atm-http-runtime` listener and one endpoint publisher for the selected
   daemon pair, with `atm doctor --json` healthy before and after the run.
4. Link each final artifact from the report master index and retain the
   artifact's proof SHA, host, operating system, architecture, and command
   outcome. Do not treat legacy pre-Tokio evidence as a substitute for these
   rows.
