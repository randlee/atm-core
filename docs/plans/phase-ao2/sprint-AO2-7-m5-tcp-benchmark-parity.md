---
phase: AO2
sprint: AO2.7
title: Full benchmark-matrix parity remediation on M5
branch: future-dev-worktree
integration_branch: integrate/phase-ao2
status: draft_for_review
depends_on:
  - AO2.5.4-mandatory-benchmark-snapshot-restore
  - AO2.6-admission-writer-batching-regression
blocks:
  - AO2.8-windows-tcp-benchmark-parity
---

# AO2.7 — Full benchmark-matrix parity remediation on M5

## Decision

`just benchmark` is one mandatory performance suite, not a convenient wrapper
around one selected transport. Every invocation must run, retain, and report
all four targets in this exact order:

1. `sqlite` — the production `atm-storage-rusqlite` admission-writer and
   transaction path, measured without HTTP, UDS, TCP, or TLS work;
2. `uds` — the released Tokio/Axum daemon's local Unix-domain public admission
   path;
3. `tcp` — the released daemon's loopback TCP public admission path with
   explicit plaintext peer-wire security; and
4. `tcp-tls` — the same TCP public admission path with ordinary mutual TLS.

The suite must never silently select, skip, or publish only one target. A
missing target, failed target, incompatible artifact, or unavailable required
platform makes the suite incomplete and non-passing. Individual diagnostic
commands may exist only below the suite API; they may not produce an
acceptance artifact or be described as `just benchmark` success.

M5 is the primary parity machine. At the fixed f8 acceptance profile its
expected medians are:

| Target | Expected M5 median |
| --- | ---: |
| `sqlite` | at least 45,000 messages/second |
| `uds` | at least 17,000 messages/second |
| `tcp` | at least 16,000 messages/second |
| `tcp-tls` | at least 16,000 messages/second |

These are explicit closure criteria, not aspirational report labels. AO2.7
does not pass while any target is below its threshold. The previous single-TCP
evidence sprint is superseded because a 6.3k TCP result cannot establish
parity and a one-target run cannot localize a regression.

## Why the matrix is mandatory

The four targets isolate the layers that must remain independent:

| Target result | Primary interpretation when it regresses |
| --- | --- |
| `sqlite` | storage writer batching, transaction count, commit/fsync, allocation, or storage contention |
| `uds` | daemon/router/serialization/received-hook overhead above the storage baseline |
| `tcp` | TCP HTTP framing, connection handling, or loopback network overhead above UDS |
| `tcp-tls` | TLS adapter/handshake/stream overhead above plaintext TCP |

No target may be used to excuse another. In particular, TLS remains a wrapper:
plaintext TCP performance must not be traded away to add TLS, and mTLS cannot
be disabled, compiled out, or benchmarked with a different request pipeline to
make the comparison look better.

## Preconditions

- AO2.5.4 is merged and the benchmark-account preflight proves a dedicated
  disposable OS account, snapshot-before-roster, and exact clean-baseline
  restore. The interactive user's `~/.atm/db` is never opened, copied,
  renamed, restored, or used as a fixture.
- AO2.6 is merged, all ordinary CI gates pass, and the exact tested
  `integrate/phase-ao2` SHA is known.
- M5 has the released, matching CLI and Tokio/Axum `atm-http-runtime` daemon
  binaries, a valid benchmark-account manifest, stable power/network state,
  and no ambient benchmark-account daemon.
- The target contract is implemented before any acceptance run: the suite has
  a direct production-writer `sqlite` measurement, UDS on every platform that
  supports it, plaintext TCP, and mTLS TCP. Platform support must be detected
  and reported before setup; it must not become an implicit skip.

## Required implementation work

### 1. Make the suite unskippable

Refactor the `just benchmark` entry point and its runner so its normal command
has no target-selection or skip option. It must build the one released binary
pair once, run all four targets, and publish one suite manifest only after all
four completed artifacts validate. A per-target helper is permitted for unit
tests and investigation but must be private to the harness or explicitly mark
its output `diagnostic_only`; it cannot satisfy an acceptance gate.

Every target run shares one immutable suite record containing: suite ID, exact
Git SHA, binary SHA-256 values, host facts, benchmark-account identity hash,
active received-hook mode, frame profile, worker/connection settings, and
peer-wire mode. The runner creates the verified clean baseline before any
roster or target, restores that exact baseline between targets, and proves the
same clean baseline after the final target. Snapshot/restore work is outside
all timed intervals.

### 2. Define the four comparable measurements

- **SQLite.** Add a production-writer benchmark seam owned by
  `atm-storage-rusqlite`, not ad-hoc raw SQL. It must exercise the same
  admission operation, writer batching, transaction, savepoint, commit, and
  reply-after-commit semantics that the daemon uses. It must report message
  count, transaction/commit count, elapsed time, p50/p95/p99, and errors.
- **UDS.** Run the released daemon's ordinary public admission request through
  its published UDS endpoint. UDS is unavailable only where the operating
  system truly lacks the public endpoint; such a platform is a blocked suite,
  not a TCP-only pass.
- **TCP.** Run the identical public request shape over loopback TCP with the
  explicit plaintext-test peer-wire mode.
- **TCP+TLS.** Run the identical TCP request shape with ordinary mutual TLS.
  It must use the same daemon admission pipeline, active hook, roster, message
  body, worker limit, and frame count as plaintext TCP.

Each target's f8 median is its acceptance measurement. Retain f1 and f2 as
mandatory connection/setup diagnostics and the remaining existing sparse
profiles as report diagnostics; none may replace the f8 threshold.

### 3. Enforce a complete artifact contract

The report schema and report builder must reject a suite without exactly one
validated record for `sqlite`, `uds`, `tcp`, and `tcp-tls`. It must include
per-target samples, p50/p95/p99, accepted/requested/error counts, transaction
and commit counts where applicable, daemon diagnostics where applicable,
snapshot/restore phase durations, baseline identity, and a matrix summary
whose threshold verdict is an AND over all four targets. A partial artifact
must be visibly `incomplete`, never `passed`.

Add deterministic tests that prove `just benchmark` invokes all four in order,
refuses a missing/duplicate/unknown target, keeps the same profile facts across
the matrix, restores the clean benchmark account between targets, and refuses
to publish a partial report.

## Remediation loop

AO2.7 is a development-and-measurement sprint, not evidence-only work.
Every iteration runs the complete matrix and reports all four target numbers,
but remediation proceeds in this strict priority order: `sqlite`, then `uds`,
then `tcp`, then `tcp-tls`. The priority target is the first target below its
threshold, or the first target with a correctness/harness failure. It stays
the priority until the issue is resolved and a subsequent complete matrix
confirms it; the other three results remain mandatory measurements on every
iteration.

For each priority target:

1. Capture the failing reproduction and the complete four-target report.
2. Trace the real released production path, quantify the dominant work, and
   compare it with the closest faster layer. Inspect every new or changed hot
   operation: batching/transaction boundaries; per-message allocations and
   copies; serialization; routing and middleware; lock or channel handoffs;
   network framing and connection behavior; logging; and TLS stream work.
3. Remove unjustified work or restore an equivalent efficient design while
   preserving the feature, request semantics, ordering, durability, typed
   failures, active hooks, and public API. The remedy is never to remove TLS,
   bypass the daemon, disable a hook, alter the workload, or use a special
   benchmark build.
4. Add or update the smallest focused regression test that proves both the
   behavior and the performance-relevant invariant, run the required
   correctness and boundary gates, then rerun the complete matrix.

A test, harness, configuration, fixture, report-schema, or ordinary runtime
failure is a defect to root-cause and repair in the current iteration, not a
reason to stop after reporting it. Its report must include the reproduction,
root cause, fix, and validation. Only a genuinely major architecture decision
or an external hardware/authority failure can pause implementation; it must
be evidenced precisely and leave the sprint blocked rather than passed.

After each full M5 matrix run:

1. Compare every f8 result with its threshold and compare adjacent layers
   (`sqlite → uds → tcp → tcp-tls`). Retain the entire raw matrix before
   changing code.
2. For every failing target, classify the limiting layer from measured facts:
   writer batch/transaction/commit behavior; daemon/router/serialization/hook;
   TCP framing/connection behavior; TLS stream/handshake behavior; logging;
   or host contention/power state. Do not change flags, disable hooks, compile
   a special binary, or change the workload to make a number pass.
3. Inspect the production hot path and make the smallest code change justified
   by that classification. Keep layer boundaries intact: storage fixes belong
   in the writer, HTTP fixes in the Tokio/Axum path, and TLS fixes in the TLS
   wrapper. A TLS change must not alter the plaintext path.
4. Run focused correctness, architecture/boundary, and full repository gates.
   Then run the whole four-target M5 matrix again; no fix is accepted from a
   single-target rerun.
5. Repeat the priority-target process until all four M5 f8 medians meet their
   thresholds. There is no arbitrary iteration cap and no conversion of a
   below-threshold result into a passing evidence report.

Every progress or closure report must present a four-row table with the suite
ID, tested SHA, priority target, each target's f8 median and threshold,
accepted/error counts, and the change or investigation since the preceding
matrix. A low number may be reported as evidence, but it must be followed by
the corresponding root-cause/fix work; it cannot be the final action.

If exhaustive profiling and source inspection leave no credible safe change,
the sprint remains **blocked, not passed**. Its closure report must include
the full matrix history, profiles, flame/trace or equivalent measurements,
rejected hypotheses, exact code paths examined, and the reason further change
would violate a documented correctness or architecture boundary. Only an
explicit product decision may alter a threshold or end that blocked state.

## Boundaries and anti-gaming rules

- Use the released CLI and Tokio/Axum daemon pair only; the frozen synchronous
  daemon is not a benchmark target.
- No benchmark-only compile feature, alternate request pipeline, hook disable,
  TLS bypass, separate daemon, alternate persistent root, environment-selected
  performance mode, or threshold waiver is allowed.
- The dedicated benchmark account is the only mutable state. The suite must
  fail before any mutation if its manifest/snapshot contract is invalid.
- Setup, snapshot, restore, report generation, source builds, and diagnostics
  are excluded from timed samples but retained as evidence.
- A production optimization must preserve write ordering, one durable response
  after successful commit, typed error/recovery context, and the existing
  Tokio/Axum crate boundaries. No change may resurrect the legacy daemon.

## Acceptance criteria

| Requirement | Required proof |
| --- | --- |
| Mandatory matrix | One ordinary `just benchmark` invocation emits exactly `sqlite`, `uds`, `tcp`, and `tcp-tls`; partial selection is rejected. |
| SQLite parity | M5 f8 median ≥45k msg/s through the production writer path. |
| UDS parity | M5 f8 median ≥17k msg/s through the released public UDS path. |
| TCP parity | M5 f8 median ≥16k msg/s through plaintext public TCP. |
| TLS parity | M5 f8 median ≥16k msg/s through the same public TCP path with mTLS. |
| Comparable inputs | All targets retain the same suite ID, SHA, binary hashes, active hook, f8 profile, request body, roster contract, and connection/worker settings. |
| Safety | Snapshot precedes the first roster; verified restore occurs between targets and after the suite; no interactive root is touched. |
| Integrity | Every target retains all samples, p50/p95/p99, accepted/errors, and target-specific transaction/commit or daemon diagnostics. |
| Closure | Every threshold passes in one complete M5 suite, or the sprint is explicitly blocked with exhaustive measured analysis; a below-threshold report is never a pass. |

Required gates are targeted Rust/Python tests, architecture/boundary guards,
`just lint`, `just test`, a full physical M5 suite artifact, and independent
review of the exact suite manifest. M4, synthetic results, loopback smoke, or
Windows evidence cannot substitute for M5 closure.

## Rollback

Harness changes roll back normally. A failed/interrupted suite stops only its
owned benchmark daemon and restores only the last verified benchmark-account
snapshot. Any production remediation reverts as its own scoped commit; no
database migration or interactive-account recovery is part of this sprint.
