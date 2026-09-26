# ADR-064 — Task Telemetry Export Through OpenTelemetry

| Field | Value |
| --- | --- |
| ID | ADR-064 |
| Status | Accepted |
| Date | 2026-09-26 |
| Scope | Task-ledger telemetry projection and OpenTelemetry export |
| Relates to | ADR-014, ADR-032, ADR-046, ADR-061, ADR-062 |

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

`TaskTelemetrySink` is a sealed, object-safe, first-party boundary with the
same three delivery errors as `WorkflowTelemetrySink`. The workflow sink is
not merged into or replaced by this contract.

### D3. Best-effort isolation

Export is best effort. A full queue, timeout, rejection, exporter failure, or
bounded-shutdown drop is diagnostic data only and cannot fail or roll back the
task operation whose durable fact is being projected.

### D4. Explicit configuration

`TelemetryExportConfig::from_env` reads only through `EnvSource`. Export is
inert when `ATM_OTEL_ENDPOINT` is absent. A configured empty endpoint or an
unsupported protocol returns `ATM_TELEMETRY_EXPORT_CONFIG_INVALID` without a
panic. The public variables are `ATM_OTEL_ENDPOINT`, `ATM_OTEL_PROTOCOL`
(`grpc` by default or `http/json`), `ATM_OTEL_AUTH_HEADER`, and
`ATM_OTEL_SERVICE_NAME` (`atm-daemon` by default).

### D5. Dependency direction

`atm-core` owns records, configuration, the sink, its no-op implementation,
and health DTOs without importing `sc-observability`. `atm-runtime` owns
composition and queue-fronting emission. `atm-observability` owns the concrete
exporter. Other crates receive runtime handles, not exporter dependencies.

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

## Consequences

- every later task exporter compiles against one typed, payload-free contract
- export outages are visible without reducing daemon availability
- runtime emission, exporter implementation, rendering, and composition land
  in later Phase BD sprints
- new task-event kinds require an explicit telemetry-kind decision

## Appendix A — Span And Metric Naming Registry

Reserved for Phase BD.6. That sprint records the exact span names, metric
names, and attribute keys exported by the concrete Phase BD.5 implementation.
