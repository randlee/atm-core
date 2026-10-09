# ADR-064 — Task Telemetry Export Through OpenTelemetry

| Field | Value |
| --- | --- |
| ID | ADR-064 |
| Status | Accepted |
| Date | 2026-09-26 |
| Scope | Task-ledger telemetry projection and OpenTelemetry export |
| Relates to | ADR-011, ADR-014, ADR-020, ADR-032, ADR-046, ADR-055, ADR-061, ADR-062 |

## Context

ATM has a durable task ledger and prompt-handoff audit trail, but no bounded,
typed contract for exporting those facts. The export path must remain a
non-authoritative projection: telemetry loss cannot affect task state,
routing, admission, retry, policy, or security. `atm-core` also cannot depend
on `sc-observability`, so it must publish an ATM-owned boundary rather than an
exporter implementation.

## Decision

### D1. ATM-owned record

`atm-core` owns `TaskTelemetryRecord`, `TaskTelemetryKind`, and
`TaskHandoffFacts`. The record carries only typed fields already present in a
durable task event or prompt handoff. It never carries a message body,
template variables, or free-form event detail.

### D2. Separate task sink

`TaskTelemetrySink` is a sealed, object-safe, first-party boundary. Its
`emit` is synchronous and returns `Unavailable` or `Rejected`; it must not
block. The task telemetry runtime's single worker calls it for each record
taken from the one bounded ATM queue, so encoding, identity and dedup stay
off the producer path, and the exporter's SDK batch processor is the only
export queue behind it. There is no emit timeout. The former workflow
telemetry runtime and `WorkflowTelemetrySink` had no production producer and
were removed (2026-10-09 ruling), leaving this one runtime.

### D3. Best-effort isolation

Export is best effort. A full queue, rejection, exporter failure, or
bounded-shutdown drop is diagnostic data only and cannot fail or roll back the
task operation whose durable fact is being projected.

### D4. Explicit configuration

`TelemetryExportConfig::from_env` reads only through `EnvSource`. Export is
inert when `ATM_OTEL_ENDPOINT` is absent. The public variables are
`ATM_OTEL_ENDPOINT`, `ATM_OTEL_PROTOCOL` (`grpc`, the default and the only
accepted value, because the exporter uses the official OpenTelemetry tonic
transport; `http/json`, `http/protobuf` and anything else are rejected with a
remediation to use `grpc`), `ATM_OTEL_AUTH_HEADER`, `ATM_OTEL_SERVICE_NAME`
(`atm-daemon` by default) and `ATM_LOG_DESTINATION` (`file` by default,
`otel` or `both`; the last two require `ATM_OTEL_ENDPOINT`).
`LogDestination::from_env` is the single parser shared by the CLI and the
daemon.

The endpoint must be an `http` or `https` URL with a host and no embedded
credentials. The auth header must be visible ASCII and at most 8192 bytes, and
it is refused over plain `http` to a non-loopback host, so an authenticated
remote collector requires `https`. Every invalid value returns
`ATM_TELEMETRY_EXPORT_CONFIG_INVALID` without a panic, and ATM continues with
export disabled.

Validated-newtype wrappers rejected; private fields + single validated constructor.
`TelemetryExportConfig` has private fields, read-only accessors
and `from_env` as its only constructor; it has no `Default`, `Deserialize` or
`Display`, and its `Debug` redacts the auth header. Health carries only the
already-validated endpoint, never the auth header.

### D5. Dependency direction

`atm-core` owns records, configuration, the sink, its no-op implementation,
and health DTOs without importing `sc-observability` or OpenTelemetry.
`atm-runtime` owns composition and queue-fronting emission. `atm-observability`
owns the concrete exporters and the domain projection onto OpenTelemetry
records.

Accepted dependency decisions:

- `atm-observability -> atm-runtime` is accepted for the setup types the
  exporter needs to register with runtime composition.
- `atm-daemon-bootstrap -> opentelemetry_sdk` is accepted only for the
  lifecycle (construction, flush, shutdown) of the standard SDK providers,
  which bootstrap holds in its existing `DaemonObservability`. Exporters and
  domain projection stay in `atm-observability`.

Other crates receive runtime handles, not exporter dependencies.

### D6. Health projection

`AtmObservabilityHealth.export` is the one doctor-visible projection of export
state and bounded counters. It carries typed state/failure enums and no
free-form exporter error. Degraded or unavailable export folds into the one
existing observability finding; it does not create a second finding.

### D7. Governed wire change

The optional export health field is an additive HTTP interface change.
`HTTP_API_VERSION` moves from 1.10.0 to 1.11.0. The field defaults to absent,
so a pinned pre-1.11 doctor payload remains readable.

### D8. Durable rows remain authoritative

Task events and prompt handoffs are exported only after their owning durable
operation succeeds. Export data is not read back to make ATM decisions and is
not a replacement for task-ledger history.

### D9. First-party boundary governance

The boundary manifest permits `atm-runtime` and `atm-observability` only,
forbids payload/variable export, and requires best-effort behavior. The seal is
the ADR-001 workspace-convention seal enforced by boundary lint and review.

### D10. One task telemetry contract

`TaskTelemetrySink` is the only telemetry sink contract; the exporter
implements it. The ADR-046 workflow sink, which had no production producer,
was removed with its runtime. This decision adds no generic telemetry
framework.

### D11. Daemon composition

The retained-log baseline is `sc-observability`, `sc-observability-log` and
`sc-observability-types` 1.5.0. Export uses the official
`opentelemetry`/`opentelemetry_sdk`/`opentelemetry-otlp` 0.33.0 crates over
gRPC (tonic). No other observability facade or HTTP exporter is composed.

- `DaemonObservability` (bootstrap) resolves configuration once at startup.
  Absent `ATM_OTEL_ENDPOINT`: no exporter, worker or provider, and `Inert`
  health. Invalid configuration or failed SDK setup: the daemon keeps
  serving with file logging, export disabled and `ConfigInvalid` health.
  Rejected values are never echoed.
- Bootstrap holds only the lifecycle handles of the three standard providers
  (traces, logs, metrics), and composition hands one task telemetry runtime
  handle to the router and the queue-wake pump. Producers call the
  non-blocking `try_emit`.
- Actual task producers: the router (assigned, reassigned, started, closed,
  reopened, rejected, prompt handoff) and the queue-wake pump (reminded,
  reminders reset, lead notified, reminder prompt handoff). Acknowledgement
  produces no row and no record.
- Bounds are the exporter constants: SDK queue `EXPORT_QUEUE` 256, batch
  `EXPORT_BATCH` 256, interval `EXPORT_INTERVAL` 1s and per-export
  `EXPORT_TIMEOUT` 400 ms, with the tonic transport bound
  `EXPORT_TRANSPORT_TIMEOUT` 300 ms below it so a stalled export always
  ends as a transport timeout (`otel_setup.rs`). The task projection keeps at
  most `ACTIVE_LIMIT` 4096 open assignments, `EVENT_LIMIT` 64 events and
  `BYTE_LIMIT` 64 KiB per assignment, drops a record over `RECORD_LIMIT`
  16 KiB, and deduplicates within a `DEDUP_LIMIT` 8192-entry window only
  (`task_exporter.rs`). A close seen before its start exports a partial
  span; nothing is replayed, backfilled or stored across an outage.
- Shutdown uses one cumulative deadline (`REPLACEMENT_DRAIN_DEADLINE`, 5s):
  listeners, recovery sweep, peers, then the task telemetry drain, then
  the providers, bounded by `min(1s, remaining)`. The first shutdown caller
  owns provider shutdown, so a cancelled or concurrent caller waits for the
  same stored outcome until its own deadline. A timeout abandons the wait,
  not the SDK call, and process exit releases it.
- Health: `Inert` with no endpoint; `Healthy` when configured and no loss or
  failure has been observed; `Degraded` when the runtime counted
  `dropped_full` or `dropped_failure`; `Unavailable` with
  `last_failure` set when the SDK reported a transport failure through the
  process-global tracing bridge, a provider shutdown failed or timed out, or
  configuration was invalid. An observed failure is not cleared by later
  success. SDK-private queue losses are unknown and never invented. The
  governed doctor JSON keeps `dropped_timeout`, which is always 0 because
  the synchronous sink has no emit timeout.
- Shutdown aborts a drain that outlives its deadline. Because the sink is
  synchronous, an aborted worker stops only after its current sink call
  returns; shutdown waits for that only until the deadline and then abandons
  the worker. Every admitted record is counted exactly once: a record already
  counted as a shutdown drop is not counted again if an abandoned sink call
  returns late.

## Consequences

- every later task exporter compiles against one typed, payload-free contract
- export outages are visible without reducing daemon availability
- the daemon exports every committed task fact while it serves, and an
  export outage changes only health, never a task result or shutdown bound
- new task-event kinds require an explicit telemetry-kind decision
