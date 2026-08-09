# AL.9 benchmark gate

**Status:** contract and immutable baseline frozen; current-runtime
measurements are pending. This is deliberately a gate, not a performance-pass
claim.

## Baseline identity

The AL baseline is the committed `develop` revision
`67401907039f92e58e883273f02372a637202f70`, captured before AL.1 added the
Tokio/Axum dependency graph. The following compact artifacts are present in
that exact Git tree and are the local macOS comparison workload:

| Transport | Frames/connection | Baseline artifact | Source revision in artifact | Throughput p50 (/s) |
| --- | ---: | --- | --- | ---: |
| UDS | 1 | `site/reports/send-message-benchmark/20260801-072313.590684-mac-arm64-01-uds-f1.json` | `3ec7ce1ff7269d8f43a65658c712778abbf2de14` | 14,715.31 |
| UDS | 2 | `site/reports/send-message-benchmark/20260801-072502.577968-mac-arm64-01-uds-f2.json` | `3ec7ce1ff7269d8f43a65658c712778abbf2de14` | 22,835.66 |
| TCP | 1 | `site/reports/send-message-benchmark/20260801-072723.571920-mac-arm64-01-tcp-f1.json` | `3ec7ce1ff7269d8f43a65658c712778abbf2de14` | 11,847.59 |
| TCP | 2 | `site/reports/send-message-benchmark/20260801-072744.295213-mac-arm64-01-tcp-f2.json` | `3ec7ce1ff7269d8f43a65658c712778abbf2de14` | 19,674.03 |

The embedded source revisions identify the binaries that produced historical
data; their presence in the pinned `67401907` tree makes the evidence a valid
pre-AL baseline. They do **not** make them a result for `atm-http-runtime`.

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

No current-runtime benchmark has been run from this worktree. The runner
correctly refuses to attach to or replace an ambient daemon and requires an
isolated OS user (or explicit idle-host backup/restore authority). This agent
has neither authority and will not alter the active host state to obtain a
measurement.

The runner selects its mode only by launching the separately compiled
`atm-daemon-benchmark --hook-mode <active|disabled>` binary. That binary is
available only with `atm-daemon-bootstrap`'s `benchmark-harness` feature; the
shipped `atm-daemon` composition always injects the active received-hook
selector and does not read a hook-mode environment variable. Build the
harness with:

```sh
cargo build --release -p atm-daemon-bootstrap \
  --features benchmark-harness --bin atm-daemon-benchmark
```

The production daemon remains unable to select `disabled`. Current-runtime
rows, including hook-active and Windows, are still **pending** until an
authorized operator executes the isolated benchmark gate.

### Explicit managed-host backup/restore mode

The default remains fail-closed: a runner must use a dedicated clean OS user
and refuses to attach to, stop, or replace an ambient daemon. An operator who
is explicitly authorized to interrupt the sole managed daemon may instead set
`ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE=1` and provide the normal
`daemon-switch` service details to the runner:

```sh
ATM_CAPACITY_BACKUP_RESTORE_HOST_STATE=1 \
python3 scripts/smoke/run_admission_capacity.py \
  --managed-service <actual-label> \
  --managed-launch-agent-plist ~/Library/LaunchAgents/<actual-label>.plist
```

On Linux or Windows, supply the service name and any documented selector-link
arguments appropriate to `daemon-switch`. This mode captures the selected
pair and its healthy doctor state, calls only `daemon-switch quiesce` to stop
the one managed daemon, atomically moves the complete host `.atm` state root,
and runs the disposable benchmark. In a `finally` path it restores that root,
restarts the same selected pair through `daemon-switch`, verifies `atm doctor`
through the switch status, and rejects selector drift. It must be used only by
an authorized operator; it does not weaken the clean-user default.

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
