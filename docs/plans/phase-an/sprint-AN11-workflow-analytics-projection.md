---
title: AN.11 Local Workflow Analytics, Query, And Telemetry Projection
status: complete
branch: feature/pan-s11-workflow-analytics-projection
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pan-s11-workflow-analytics-projection
target: integrate/phase-an
---

# AN.11 — Local Workflow Analytics, Query, And Telemetry Projection

**recommended_agent:** arch-ctm/deep-reasoning (generic pairing semantics and
public read contracts).
**must_follow:** AN.10 merged; merge its integration tip before every dev/fix
round because query semantics consume its durable snapshot contract.
**unblocks:** AN.12.
**parallel_safe:** none. The query/telemetry projection shares public data
shapes and validation fixtures.

**traceability:** ADR-046; `REQ-P-WORKFLOW-ANALYTICS-001` and
`REQ-CORE-WORKFLOW-ANALYTICS-001`.

## Deliverables

1. Extend the existing bounded **local-only** CLI and HTTP typed filter
grammar with exact workflow fields and effective-tag filters. Do not accept
raw SQL, raw FTS syntax, arbitrary expressions, joins, or remote peer ingress
on HTTP. Keep Maturin's read-only `atm_query` escape hatch parameterized and
limited to its existing local read-only connection policy. Extend the existing
bounded count/group aggregates to count matching workflow facts by an
allowlisted exact workflow dimension: **only** `scope_kind`, `state`, `stage`,
or `transition`. `scope_id` and `iteration` are exact bounded filters, never
HTTP/CLI group keys; this prevents an arbitrary-cardinality aggregate response.
Callers count a selected scope/iteration or use the lifecycle projection for
per-scope work, without adding a named ATM workflow.
2. Add a generic lifecycle pairing request/outcome that has no named ATM
workflow vocabulary:

   ```rust
   pub struct WorkflowSelector {
       pub state: Option<WorkflowState>,
       pub stage: Option<WorkflowStage>,
       pub transition: Option<WorkflowTransition>,
   }

   pub struct WorkflowProjectionRequest {
       pub scope_kind: WorkflowScopeKind,
       pub scope_id: Option<WorkflowScopeId>,
       pub start: WorkflowSelector,
       pub end: WorkflowSelector,
       pub time_range: Option<TimeRange>,
   }

   pub enum LifecycleObservation {
       Completed { start: WorkflowFact, end: WorkflowFact, duration: Duration },
       Incomplete { start: WorkflowFact },
   }
   ```

   Define and test a deterministic pairing rule: within each scope, order by
   durable message timestamp and immutable row tie-breaker; each matching end
   pairs once with the earliest preceding still-unpaired matching start. A
   request rejects an empty start/end selector and impossible time bounds.
3. Expose stored tag provenance explicitly in the local result model:
   `instance_tags`, `applied_template_tags`, `derived_tags`, and
   `effective_tags`; calculate `derived_tags` from immutable snapshot fields,
   not a caller-writable stored array, and do not force callers to infer
   provenance from a union.
4. Add the **sealed, object-safe** `atm_core::workflow_telemetry::
   WorkflowTelemetrySink` and leaf records/errors specified in ADR-046. The
   trait is first-party only, injected as `Arc<dyn WorkflowTelemetrySink>`;
   it is not a public third-party plug-in contract. The AN.11 PR must add the
   synchronized boundary records
   `boundaries/atm-core/workflow-telemetry-sink.toml` and its
   `docs/atm-core/boundaries.md` section. `atm-core` owns the contract;
   `atm-core::NoopWorkflowTelemetrySink` is the built-in default and
   `atm-runtime` is the sole allowed out-of-owner implementation/composition
   site; `atm`, `atm-http-runtime`, `atm-storage`, and
   `atm-storage-rusqlite` must not construct exporters or depend on telemetry
   implementation crates.

   The default sink is inert. The supervised `atm-runtime` worker receives
   records through a non-blocking bounded Tokio channel (default 256; range
   1–4,096), applies the ADR-046 timeout (default one second; configured range
   1 ms–30 s), and
   reports queue-full, timeout, emit failure, and bounded-shutdown drops as
   diagnostics/counters. It owns shutdown: close intake, drain through its
   configured drain deadline (default two seconds; range 1 ms–30 s), then
   cancel; no detached export task survives. Startup
   validates exporter config before construction and selects the no-op sink on
   invalid config while doctor reports degraded telemetry. A configured exporter
   receives only scope/snapshot attributes and stored timestamps—never message
   payloads or merged vars. No telemetry result may reject admission, alter
   routing/retry/security, or change query results.

   Register the workflow-query and telemetry configuration codes from
   ADR-046 in `docs/atm-error-codes.md`. Query validation returns typed
   `AtmError`; runtime-worker failures remain structured diagnostics/counters,
   never delayed command failures.

## Acceptance criteria

- Local CLI/HTTP can filter by every workflow snapshot field and effective
  tag; peer ingress rejects the route before storage access.
- The local Python/Maturin read-only surface can issue a parameterized query
  over the documented decomposed view and return the exact provenance fields.
- Pairing is deterministic for simultaneous timestamps, repeated starts/ends,
  incomplete cycles, and two unrelated workflow vocabularies.
- HTTP/CLI aggregate output can group only by `scope_kind`, `state`, `stage`,
  or `transition`; exact `scope_id`/`iteration` filters cannot expand result
  cardinality. AN.12's hand-computed fixtures cover every permitted dimension.
- Telemetry is disabled without configuration and records no payload/merged
  variable data when enabled. A full queue, timeout, invalid config, shutdown,
  and deliberately failing sink cannot affect admission or routing.
- No API reserves values such as `dev`, `qa`, `fix`, or `sprint`.

## Required validation

- unit/property tests for selector validation and one-to-one deterministic
  pairing
- CLI, local UDS/loopback HTTP, and peer-ingress-rejection contract tests
- Python parameterization/read-only tests using `decomposed_messages`
- no-op, full-queue, timeout, invalid-config, bounded-shutdown, and
  failing-sink isolation tests
- `cargo test` for affected crates, boundary lint, and `just test`

## Paths to delete

None. Existing AN.6 filters and raw local read-only Python queries remain
compatible; workflow selectors are additive.

## Non-closure

AN.11 does not ship a remote analytics API, a default external exporter,
historical backfill, cross-host template synchronization, or a workflow engine.
