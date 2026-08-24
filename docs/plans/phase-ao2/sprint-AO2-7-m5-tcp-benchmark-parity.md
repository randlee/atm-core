---
phase: AO2
sprint: AO2.7
title: Full benchmark-matrix harness and M5 artifact contract
branch: future-dev-worktree
integration_branch: integrate/phase-ao2
status: draft_for_review
depends_on:
  - AO2.5.4-mandatory-benchmark-snapshot-restore
  - AO2.6-admission-writer-batching-regression
dependency_relations:
  - prerequisite: AO2.5.4
    relation: must_follow
  - prerequisite: AO2.6
    relation: must_follow
blocks:
  - AO2.8-m5-benchmark-parity-remediation
parallel_safe_with: []
---

# AO2.7 — Full benchmark-matrix harness and M5 artifact contract

## Decision and bounded scope

AO2.7 is a bounded harness-and-contract sprint. It makes `just benchmark` run
and report exactly four comparable targets in order: `sqlite`, `uds`, `tcp`,
and `tcp-tls`. It does **not** claim performance parity; AO2.8 owns measured
M5 remediation. This split prevents an unbounded optimization investigation
from hiding an unfinished harness implementation.

The ordinary command has no target selection or skip mode. Private diagnostic
helpers may exist for tests and profiling, but their output is
`diagnostic_only` and can never be used as an acceptance artifact. A missing,
duplicate, failed, incompatible, or unavailable required target yields an
`incomplete` suite, never success.

AO2.7 depends on AO2.5.4's dedicated-account snapshot/restore contract and
AO2.6's writer batching repair. It blocks AO2.8. No work is parallel-safe with
AO2.8 because the latter consumes the exact contract created here.

## Frozen f8 workload

`f8-v1` is the sole acceptance workload until an explicit product decision
changes both this ID and the performance thresholds. It uses the released CLI
and Tokio/Axum `atm-http-runtime` daemon, the public `/v1/atm/messages`
request, active received hook, 8 request frames per connection, 1,000 requests
per interval, exactly 10 independent intervals, exactly 20 timed seconds, 64 workers,
and the existing bounded eight-in-flight HTTP/1.1 pipeline. The exact
versioned request-body builder and its SHA-256 are recorded in every artifact.
Every field of `F8Profile` is frozen for all attempts against a candidate
revision. No iteration may change message content/size, roster semantics,
worker count, connection behavior, logging level, hook, TLS mode, build
features, or daemon implementation while retaining `f8-v1`.

`sqlite` measures the production `atm-storage-rusqlite` admission writer;
`uds` uses the released public UDS endpoint; `tcp` uses the same public TCP
endpoint in explicit plaintext-test mode; and `tcp-tls` uses that same TCP
endpoint with ordinary mTLS. TLS remains a wrapper: it must not alter the
plaintext request pipeline.

## Normative contract

The Python runner may mirror this contract with Pydantic, but these Rust types
are normative for field names, invariants, and the AO2.8 consumer. They belong
to a private benchmark-contract module; no public extension trait or new
production daemon boundary is introduced.

```rust
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
enum BenchmarkTarget { Sqlite, Uds, TcpPlaintext, TcpTls }

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct F8Profile {
    id: WorkloadId,                   // validated "f8-v1"
    frames_per_connection: NonZeroU16, // 8
    requests_per_interval: NonZeroU32, // 1_000
    minimum_interval_count: NonZeroU16, // 10
    minimum_timed_seconds: NonZeroU16,  // 20
    worker_limit: NonZeroU16,           // 64
    max_in_flight: NonZeroU16,          // 8
    request_body_sha256: [u8; 32],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TargetResult {
    target: BenchmarkTarget,
    median_msg_per_second: f64,
    p95_msg_per_second: f64,
    p99_msg_per_second: f64,
    requested: u64,
    accepted: u64,
    errors: u64,
    raw_artifact_sha256: [u8; 32],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TargetThreshold {
    target: BenchmarkTarget,
    expected_msg_per_second: Decimal,
    closure_floor_msg_per_second: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct HostTelemetry {
    logical_cpu_count: NonZeroU16,
    load_average_1m: Decimal,
    competing_process_cpu_percent: Decimal,
    benchmark_process_cpu_percent: Decimal,
    available_memory_bytes: u64,
    free_disk_bytes: u64,
    kernel_release: KernelRelease,
    power_mode: PowerMode,
    sample_interval_seconds: Decimal,
    observation_duration_seconds: Decimal, // >= 10
    competing_cpu_at_or_above_20_percent_seconds: Decimal,
    load_above_125_percent_cpu_seconds: Decimal,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SuiteIntent {
    sequence: NonZeroU32, // append and fsync before the suite starts
    suite_id: SuiteId,
    started_at: UtcTimestamp,
    candidate_revision: GitRevision,
    production_revision: GitRevision,
    harness_revision: GitRevision,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CandidateLineage {
    prior_candidate_revision: GitRevision,
    prior_ledger_sha256: [u8; 32],
    reviewed_at: UtcTimestamp,
    disposition: ReviewedFailedOrIncomplete | AcceptedBaseline,
    rationale: NonEmptyString,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CompleteSuiteAttempt {
    sequence: NonZeroU32, // must reference one durable SuiteIntent
    suite_id: SuiteId,
    started_at: UtcTimestamp,
    completed_at: UtcTimestamp,
    candidate_revision: GitRevision,
    production_revision: GitRevision,
    harness_revision: GitRevision,
    results: [TargetResult; 4],
    snapshot: VerifiedSnapshotId,
    restore_verified: bool,
    telemetry_before: HostTelemetry,
    telemetry_after: HostTelemetry,
    raw_artifact_sha256: [u8; 32],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct M5AttemptLedger {
    schema_version: u16,
    candidate_revision: GitRevision,
    host: BenchmarkHost,
    f8: F8Profile,
    thresholds: [TargetThreshold; 4],
    lineage: Option<CandidateLineage>,
    intents: Vec<SuiteIntent>, // every started suite, persisted before execution
    attempts: Vec<CompleteSuiteAttempt>, // every completed attempt, pass or fail
    same_revision_rerun_checkpoint: Option<RerunCheckpoint>,
    accepted_m5: bool,                   // validated derivative; never caller-set
}

impl M5AttemptLedger {
    fn derive_accepted_m5(&self) -> bool; // final 3 error-free entries meet thresholds
    fn validate(&self) -> Result<(), BenchmarkError>; // checks intent/completion pairing, durations, ordering, and derived mismatch
}

pub(crate) trait SuiteManifestStore {
    fn append_completed_attempt(&self, ledger: &mut M5AttemptLedger, attempt: CompleteSuiteAttempt)
        -> Result<ManifestPath, BenchmarkError>;
    fn load_accepted_m5(&self, revision: GitRevision) -> Result<M5AttemptLedger, BenchmarkError>;
}
```

`SuiteId`, `GitRevision`, `BenchmarkHost`, `VerifiedSnapshotId`, `WorkloadId`, and
`ManifestPath` are validated newtypes (`RBP-004`), not interchangeable strings.
`SuiteManifestStore` is crate-private with one file-backed implementation, so
it is not a downstream extension point and must not modify the project's
sealed-trait topology. `BenchmarkError` must preserve a stable failure code, cause,
and recovery action (`RBP-001`). The snapshot lifecycle is dynamic and already
runtime-owned, so AO2.7 must not add a public typestate API merely for the
reporting contract (`RBP-002`).

## Required implementation and tests

1. Refactor the default `just benchmark` runner to build one released binary
   pair, execute all four targets in the fixed order, snapshot before roster,
   restore between targets and finally, and refuse partial publication.
2. Implement the direct production-writer `sqlite` measurement without raw
   ad-hoc SQL. It must exercise the normal admission, writer batching,
   transaction/savepoint, commit, and reply-after-commit behavior.
3. Implement the versioned JSON schema mirroring `M5AttemptLedger` and
   validate exact target order, one result per target, immutable `f8-v1`, all
   raw hashes, typed duration-bearing before/after telemetry, thresholds,
   contiguous sequence, and verified restoration. Append and fsync a
   `SuiteIntent` **before** each suite starts, then append its matching
   completion; an interrupted suite remains visible as an unmatched intent and
   blocks acceptance. Every complete M5 attempt (pass **and** fail) is retained;
   failed complete attempts may never be discarded or replaced.
4. Publish the M5 attempt ledger only at
   `docs/plans/phase-ao2/artifacts/ao2-7-m5-suite-<candidate_revision>.json`.
   The committed artifact must name the post-merge `integrate/phase-ao2` code
   SHA it actually tested, the harness SHA, host facts, the full profile, and
   every raw evidence path/hash. A new candidate cannot silently reset an
   unresolved series: its ledger must name and hash a reviewed prior candidate
   ledger in `CandidateLineage`. A rerun appends a new suite ID inside the same
   candidate ledger, never overwrites evidence. `accepted_m5` is derived only
   by `derive_accepted_m5` from the contiguous final three complete, error-free
   attempts; loader validation recomputes it and rejects a serialized mismatch.
   A same-production-revision recovery after a failed suite requires an
   explicit rerun checkpoint naming the failed and recovery sequences.
5. Add deterministic tests that reject target selection/skip, missing/duplicate
   targets, profile drift, wrong candidate SHA, absent raw evidence, failed
   restore, short/noisy telemetry observations, unmatched intent, unreviewed
   candidate lineage, per-attempt harness-revision mismatch, same-revision
   lucky rerun, and a Windows consumer trying to load a missing/mismatched M5
   artifact. Test that a hand-edited `accepted_m5: true`, an omitted failed
   attempt, an above-floor attempt with errors, or three non-contiguous passing
   attempts is rejected. Test the direct writer's transaction/commit counters.

## AO2.7 acceptance criteria

| Property | Required proof |
| --- | --- |
| Complete matrix | Plain `just benchmark` creates four ordered results; a subset is rejected. |
| Comparable workload | Every ledger attempt validates every exact `f8-v1` field and body hash. |
| Safety | Dedicated-account snapshot precedes roster; verified restore occurs between/finally; no interactive root is opened. |
| Machine handoff | The schema-versioned ledger is published only after testing the post-merge `integrate/phase-ao2` SHA at the fixed path; it retains every started and complete attempt and fails closed on SHA/profile/hash/lineage/telemetry/derived-acceptance mismatch. |
| Rust shape | The typed contract above is implemented/mirrored without an open public trait or new daemon boundary. |
| Gates | Focused runner/schema tests, architecture guards, `just lint`, and `just test` pass. |

## Risks and rollback

This sprint changes harness/report code only. It does not alter timed
production routing, TLS, client framing, or storage behavior. A failure in an
ordinary harness/configuration/schema test is fixed in this sprint with a
reproduction, root cause, repair, and validation; it is not reported as a
terminal benchmark result. Rollback is a scoped harness revert; AO2.5.4's
database-safety refusal remains in force.
