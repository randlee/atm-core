---
title: AI.40 local transport throughput evidence
status: in_progress
branch: feature/pAI-s40-local-transport-benchmark
recommended_agent: arch-ctm
recommended_model: deep-reasoning
execution_mode: after_merge
execution_dependencies:
  - AI.39
dependencies_relation:
  - sprint: AI.39
    relation: must_follow
    rationale: Measures the shared framing path AI.39 changes.
target: integrate/phase-ai-31-33
depends_on: AI.39
---

# AI.40 — Local transport throughput evidence

## Recommended Agent / Model

`arch-ctm` / deep-reasoning: this is performance-critical real-daemon
measurement with platform-parity and evidence-integrity constraints. This is a
planning-time recommendation, not a binding assignment.

## Execution Dependencies

AI.40 `must_follow`s AI.39. Merge-forward trigger: AI.39 development is
pushed, not QA; before every round merge it into this branch. PR-completion
trigger: AI.39's PR merges into `integrate/phase-ai-31-33` first.

## Dependency Relations

| Sprint | Relation | Rationale |
| --- | --- | --- |
| AI.39 | must_follow | Its completed shared frame reader is the benchmark subject. |

```yaml
plan_type: sprint_plan
phase: AI
sprint: AI.40
worktree: feature/pAI-s40-local-transport-benchmark
branch: feature/pAI-s40-local-transport-benchmark
status: in_progress
estimated_scope: one isolated benchmark runner and validated result schema
```

## Goal

Prove the release-built daemon sustains the required 1,000 local admissions per
second without hiding transport differences. Unix evidence measures UDS and
loopback TCP separately; Windows measures loopback TCP. AI.40 returns a
complete schema-valid result from a disposable SQLite database; AI.49 persists
the public report artifact.

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

2. Define the schema-valid result emitted by each approximately 20-second run.
   One result has one safe host label, transport, and frames-per-connection
   profile; AI.49 owns persistence and HTML rendering:

   ```json
   {
     "schema_version": 2,
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
   connection, each with at least ten independent 1K-message samples and a
   minimum 20-second sustained duration. Add explicit 10K
   and 100K sustained modes after the sparse baseline; retain queue growth,
   failure cause, and final cleanup/daemon health instead of truncating a run.

4. Return the complete result, including failed-run diagnostics, to AI.49.
   AI.40 neither writes `site/` artifacts nor renders HTML.

5. The runner rejects an ambient/shared daemon and production database. It
   records release paths, doctor result, and endpoint; cleanup restores prior
   host state after a failed sample. It may not use a mock router, direct
   dispatcher, or disabled storage write.

6. Run the release-built benchmark through SSH on M5 using its isolated ATM
   home/database. Collect both UDS and loopback TCP for one frame and every
   multi-frame profile. Before AI.39 merges, record the M5 one-frame UDS
   median as the comparison baseline using the same command, duration, and
   sample count.

## Required validation

- Unit tests cover transport validation, schema fields, profile partitioning,
  and failure retention.
- A controlled real-daemon test runs one 1K sample over every transport
  supported by that host and confirms each accepted write is durable admission.
- Unix CI proves separate UDS and TCP records; Windows rejects UDS selection
  and produces TCP evidence.
- Response parsing and application-wire byte accounting are identical between
  transports; no result body may be discarded to inflate throughput.
- On M5 over SSH, run the isolated release-built UDS and TCP profiles and
  retain all ten-sample results plus the pre-AI.39 one-frame UDS baseline.
- Run `just test`, `just lint`,
  `ATM_CAPACITY_ISOLATED_OS_USER=1 just benchmark --transport uds`, and
  `ATM_CAPACITY_ISOLATED_OS_USER=1 just benchmark --transport tcp`. The last
  two require a dedicated clean OS account or explicitly backed-up/restored
  disposable host state.

## Acceptance criteria

- Every supported transport has ten consecutive 1K-message intervals at or
  above 1,000 accepted admissions/responses per second on a release-built
  daemon and disposable SQLite database.
- On M5, median UDS one-frame throughput is at least the recorded pre-AI.39
  median; each UDS multi-frame profile exceeds M5 UDS one-frame throughput.
  TCP one- and two-frame profiles retain ten clean 1K samples at the admission
  floor and at least 75% of corresponding UDS; eight or more frames retain
  at least 90% of corresponding UDS.
- Missing M5 evidence, a missing baseline, or any threshold miss fails this
  sprint; retained diagnostics do not qualify it as complete.
- Sparse and sustained results state their transport and do not conflate TCP
  frames with kernel packet counts.
- Timeout, partial acceptance, dirty host, or failed cleanup is FAIL with
  retained diagnostics; it cannot be summarized as a passing median.

## Non-goals

No production-database benchmark, site/report rendering, OS tuning to mask
software faults, or throughput claim based on mock/direct-dispatch results.
This sprint closes local admission only, not remote HTTPS capacity.
