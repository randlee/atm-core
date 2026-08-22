---
phase: AO2
sprint: AO2.7
title: M5 TCP benchmark parity evidence after writer batching restoration
branch: future-evidence-worktree
integration_branch: integrate/phase-ao2
status: draft_for_review
must_follow: AO2.9-benchmark-report-template-and-procedure merged to integrate/phase-ao2
parallel_safe: false
depends_on:
  - AO2.5.4-mandatory-benchmark-snapshot-restore
  - AO2.6-admission-writer-batching-regression
  - AO2.9-benchmark-report-template-and-procedure
blocks:
  - AO2.8-windows-tcp-benchmark-parity
---

# AO2.7 — M5 TCP benchmark parity evidence after writer batching restoration

## Decision

Run the canonical physical TCP benchmark on M5 only after AO2.5.4 and AO2.6
have merged into `integrate/phase-ao2`. The acceptance threshold is a median
of more than 15,000 messages/second for the fixed TCP batching-comparison
profile. This sprint changes no production code unless a separately planned
finding is opened; it records reproducible evidence or a failure report.

The threshold applies to TCP with eight frames per connection. That profile is
the batching comparison: historical M5 TCP evidence recorded approximately
22.5k msg/s at eight frames, while one-frame TCP is dominated by connection
setup and historically measured about 12.3k. One- and two-frame data remains
mandatory diagnostics but is not substituted for the batching threshold.

### Reporting contract

Use the normative AO2.9 benchmark reporting, publication, and aggregate
procedure (`sprint-AO2-9-benchmark-report-template-and-procedure.md`). This
sprint owns the M5 TCP f8 threshold and safety gates; AO2.9 owns the template,
per-run path, failure/incomplete publication rule, and index contract. Publish
this run as the `tcp` target with its raw evidence and calculated result.

## Preconditions

- AO2.5.4 is merged and `just benchmark` proves snapshot-before-roster and
  restore-to-clean-baseline for the dedicated benchmark account.
- AO2.6 is merged, all normal CI gates pass, and the exact tested SHA is known.
- M5 has the released paired CLI/`atm-http-runtime` daemon build, a dedicated
  benchmark OS account/manifest, and no interactive-account benchmark mode.
- The test uses the same released binary pair, host label, profile parameters,
  raw-artifact schema, active hook mode, and peer-wire mode documented in the
  invocation evidence. It must not silently alter log level, TLS mode, worker
  count, or connection count.

## Required procedure

1. Record host facts: hostname, CPU/OS summary, exact Git SHA, release binary
   hashes, benchmark-account identity, and the selected TCP profile.
2. Run the mandatory AO2.5.4 preflight/snapshot lifecycle. Confirm the raw
   evidence identifies the clean snapshot before roster creation.
3. Run the standard TCP profile with `frames_per_connection=8`, active hook
   mode, and the ordinary peer-wire mode specified by the current benchmark
   contract. Do not use a benchmark-only compile flag or disabled hook.
4. Retain all samples, p50/p95/p99, accepted count, error count, daemon logs,
   raw artifact, compact report, snapshot/restore phase durations, and final
   clean-baseline proof.
5. Mark success only if the p50 is strictly greater than 15,000 msg/s and no
   timed sample overlaps setup/restore. Retain the one- and two-frame TCP
   profiles as diagnostics under the same run metadata.

## Failure triage contract

A result below 15k is evidence, not permission to tune ad hoc. The report must
first classify whether the failing factor is: benchmark lifecycle contamination,
binary/commit mismatch, profile mismatch, host contention/power state, writer
batch size/commit count, HTTP framing/connection behavior, logging, or another
measured component. It must include the raw evidence and a narrow hypothesis.
Any production fix requires a new plan/worktree; AO2.7 itself remains
evidence-only.

## Acceptance criteria

| Requirement | Evidence |
| --- | --- |
| Safety | Dedicated-account preflight, snapshot-before-roster, and successful exact restore. |
| Reproducibility | Exact SHA, binary hashes, M5 hostname, profile, hook and wire mode, and raw artifact retained. |
| Throughput | TCP f8 p50 >15,000 messages/second. |
| Integrity | Accepted count, durable restart check, no unexpected errors, and clean baseline after restore. |
| Diagnostics | TCP f1/f2 results retained but not confused with the f8 batching threshold. |
| Publication | Published via the AO2.9 finalizer on `evidence/ao2-benchmark-reports`, reachable through its reviewed Pages PR, with intent/result/index commits retained. |

Required gates are no code changes, successful existing test suite at the
tested SHA, the physical raw artifact, and independent review of the exact
report. If M5 is unavailable, the sprint remains blocked rather than being
passed with M4, loopback, synthetic, or Windows evidence.
