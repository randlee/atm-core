---
title: AI.40 local transport throughput evidence
status: proposed
branch: feature/pAI-s40-local-transport-benchmark
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_mode: after_merge
execution_dependencies:
  - AI.39
  - AI.46
dependencies_relation:
  - sprint: AI.39
    relation: must_follow
    rationale: Measures the shared framing path AI.39 changes.
  - sprint: AI.46
    relation: must_follow
    rationale: Emits AI.46's generated report-index contract after every run.
target: integrate/phase-ai-31-33
depends_on: AI.39, AI.46
---

# AI.40 — Local transport throughput evidence

## Recommended Agent / Model

`arch-ctm` / deep-reasoning: this is performance-critical real-daemon
measurement with platform-parity and evidence-integrity constraints. This is a
planning-time recommendation, not a binding assignment.

## Execution Dependencies

AI.40 `must_follow`s AI.39 and AI.46. Merge-forward trigger: both development
pushes, not QA; before every round merge both into this branch. PR-completion
trigger: both PRs merge into `integrate/phase-ai-31-33` first. It measures
AI.39's shared framing implementation.

The softer merge-forward-only/backfill alternative is rejected: AI.40 cannot
test its artifact writer or PR gate against the required index command before
AI.46 merges, and a later backfill can leave published evidence unindexed.

## Dependency Relations

| Sprint | Relation | Rationale |
| --- | --- | --- |
| AI.39 | must_follow | Its completed shared frame reader is the benchmark subject. |
| AI.46 | must_follow | It owns the generated report-index command invoked after every run. |

```yaml
plan_type: sprint_plan
phase: AI
sprint: AI.40
worktree: feature/pAI-s40-local-transport-benchmark
branch: feature/pAI-s40-local-transport-benchmark
status: proposed
estimated_scope: one isolated benchmark runner and checked-in evidence schema
```

## Goal

Prove the release-built daemon sustains the required 1,000 local admissions per
second without hiding transport differences. Unix evidence measures UDS and
loopback TCP separately; Windows measures loopback TCP. Every result is
durable JSON produced against a disposable SQLite database.

## Governing requirements and ADRs

- `REQ-CORE-TRANSPORT-005B`
- ADR-026 — host-owned daemon state
- ADR-033 — HTTP endpoint contract
- ADR-035 — canonical write ingress
- `.just/build_view_site.py` / `artifacts/view` ToolPanel contract; extend it,
  do not add a second generic report renderer.

## Deliverables

1. Extend AI.33's `scripts/smoke/run_admission_capacity.py`; do not create a
   second admission gate. `just benchmark` selects its profiles and reports,
   accepts explicit `--transport uds|tcp` on Unix and `--transport tcp` on
   Windows. The same public authenticated
   `POST /v1/atm/messages` request, response handling, message count, worker
   limit, timeout policy, and disposable daemon/database lifecycle apply to
   every selected transport.

2. Persist one schema-versioned JSON artifact for each approximately 20-second
   run below
   `site/reports/send-message-benchmark/<utc>-<host>-<transport>-<frames>.json`.
   One run has one host, transport, and frames-per-connection profile:

   ```json
   {
     "schema_version": 2,
     "report_type": "benchmark",
     "generated_at": "2026-07-31T19:00:00Z",
     "report_html": "../send-message-benchmark.html",
     "host_label": "mac-arm64-01",
     "transport": "uds",
     "frames_per_connection": 16,
     "run_duration_s": 20
   }
   ```

   The artifact records messages per connection, connection count, accepted and
   requested messages, elapsed time, normalized time-to-send-1K, HTTP request
   frames/sec, connections/sec, application-wire request/response/total bytes
   and bytes/sec, p50/p95/max latency, first failure, and PASS/FAIL. It calls
   the metric HTTP request frames, not IP packets: TCP segmentation is
   kernel-dependent and not inferred from application writes.

3. Default sparse profiles are exactly 1, 2, 8, 16, and 64 messages per
   connection, each with ten independent 1K-message samples. Add explicit 10K
   and 100K sustained modes after the sparse baseline; retain queue growth,
   failure cause, and final cleanup/daemon health instead of truncating a run.

4. Generate `site/reports/send-message-benchmark.html` from the immutable JSON
   directory. Its same-named directory holds JSON and per-run XHTML panels; it
   aggregates records by UTC timestamp for regression trends. `site/reports`
   is durable evidence, unlike transient `artifacts/view`; register one
   ToolPanel link in `.just/build_view_site.py`, without copying or re-rendering
   the report there. Migrate existing evidence with `transport: tcp` rather
   than leaving schema-ambiguous rows.

   Invoke `just reports-index` after every artifact write, including a failed
   run, so `site/reports/index.html` never needs manual maintenance; producer
   tests prove this invocation.

5. The runner rejects an ambient/shared daemon and production database. It
   records release paths, doctor result, and endpoint; cleanup restores prior
   host state after a failed sample. It may not use a mock router, direct
   dispatcher, or disabled storage write.

## Required validation

- Unit tests cover transport validation, schema fields, JSON migration, report
  rendering, profile partitioning, and failure retention.
- A controlled real-daemon test runs one 1K sample over every transport
  supported by that host and confirms each accepted write is durable admission.
- Unix CI proves separate UDS and TCP records; Windows rejects UDS selection
  and produces TCP evidence.
- Response parsing and application-wire byte accounting are identical between
  transports; no result body may be discarded to inflate throughput.
- Run `just test`, `just lint`,
  `just reports-index --check`,
  `ATM_CAPACITY_ISOLATED_OS_USER=1 just benchmark --transport uds`, and
  `ATM_CAPACITY_ISOLATED_OS_USER=1 just benchmark --transport tcp`. The last
  two require a dedicated clean OS account or explicitly backed-up/restored
  disposable host state.

## Acceptance criteria

- Every supported transport has ten consecutive 1K-message intervals at or
  above 1,000 accepted admissions/responses per second on a release-built
  daemon and disposable SQLite database.
- Sparse and sustained JSON records state their transport and do not conflate
  TCP frames with kernel packet counts.
- Timeout, partial acceptance, dirty host, or failed cleanup is FAIL with
  retained diagnostics; it cannot be summarized as a passing median.

## Non-goals

No production-database benchmark, OS tuning to mask software faults, or
throughput claim based on mock/direct-dispatch results. This sprint closes
local admission only, not remote HTTPS capacity.
