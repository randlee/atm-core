---
title: AN.11 Local Workflow Analytics, Query, And Telemetry Projection
status: planned
branch: feature/an11-workflow-analytics-projection
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
allowlisted exact workflow dimension, so callers can count iterations or QA
rounds without adding a named ATM workflow.
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
4. Define a no-op-by-default, dependency-inverted `WorkflowTelemetrySink` for
   OpenTelemetry-compatible span/event records derived only from completed or
   incomplete stored observations. The default sink is inert; any configured
   exporter receives scope/snapshot attributes and stored timestamps, never
   message payloads/vars unless a future explicit redaction contract permits
   them. Sink failure is reported diagnostically and cannot reject admission,
   alter routing/retry/security, or change query results.

## Acceptance criteria

- Local CLI/HTTP can filter by every workflow snapshot field and effective
  tag; peer ingress rejects the route before storage access.
- The local Python/Maturin read-only surface can issue a parameterized query
  over the documented decomposed view and return the exact provenance fields.
- Pairing is deterministic for simultaneous timestamps, repeated starts/ends,
  incomplete cycles, and two unrelated workflow vocabularies.
- Telemetry is disabled without configuration and records no payload/merged
  variable data when enabled. A deliberately failing sink cannot affect
  admission or routing.
- No API reserves values such as `dev`, `qa`, `fix`, or `sprint`.

## Required validation

- unit/property tests for selector validation and one-to-one deterministic
  pairing
- CLI, local UDS/loopback HTTP, and peer-ingress-rejection contract tests
- Python parameterization/read-only tests using `decomposed_messages`
- no-op and failing-sink isolation tests
- `cargo test` for affected crates, boundary lint, and `just test`

## Paths to delete

None. Existing AN.6 filters and raw local read-only Python queries remain
compatible; workflow selectors are additive.

## Non-closure

AN.11 does not ship a remote analytics API, a default external exporter,
historical backfill, cross-host template synchronization, or a workflow engine.
