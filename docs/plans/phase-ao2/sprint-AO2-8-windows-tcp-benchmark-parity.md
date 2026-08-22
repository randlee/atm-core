---
phase: AO2
sprint: AO2.8
title: Windows TCP benchmark parity evidence against accepted M5 result
branch: future-evidence-worktree
integration_branch: integrate/phase-ao2
status: draft_for_review
must_follow: AO2.9 and AO2.7 merged to integrate/phase-ao2
parallel_safe: false
depends_on:
  - AO2.5.4-mandatory-benchmark-snapshot-restore
  - AO2.6-admission-writer-batching-regression
  - AO2.7-m5-tcp-benchmark-parity
  - AO2.9-benchmark-report-template-and-procedure
---

# AO2.8 — Windows TCP benchmark parity evidence against accepted M5 result

## Decision

After AO2.7 establishes one accepted M5 TCP f8 result, run the same physical
benchmark on fastpc4/Windows. Windows passes when its TCP f8 median is at
least 80% of the accepted AO2.7 M5 TCP f8 median, with the same source SHA,
binary pair, benchmark profile, and evidence contract. The comparison is
explicitly cross-hardware and therefore a parity floor, not a claim that the
two machines have identical absolute performance.

For example, if AO2.7 records 15,500 msg/s, the Windows floor is 12,400 msg/s.
The exact floor must be calculated from the committed AO2.7 artifact and
recorded in the Windows raw/compact evidence; it must never be hardcoded.

### Reporting contract

Use the normative AO2.9 benchmark reporting, publication, and aggregate
procedure (`sprint-AO2-9-benchmark-report-template-and-procedure.md`). This
sprint owns the Windows parity calculation and safety gates; AO2.9 owns the
template, per-run path, failure/incomplete publication rule, and index
contract. Publish this run as the Windows `tcp` target and retain the AO2.7
reference artifact and calculated floor in the run JSON.

## Preconditions

- AO2.7 has passed on M5 with a reviewed f8 raw artifact and known p50.
- The same merged `integrate/phase-ao2` SHA is built as the matched Windows
  CLI and Tokio/Axum daemon pair.
- Windows has its own dedicated benchmark OS account and validated manifest.
  It must not use an alternate data root, `ATM_HOME` trick, interactive user,
  or a second daemon.
- AO2.5.4's mandatory snapshot/restore lifecycle is available on Windows and
  has succeeded locally before the timed profile begins.

## Required procedure

1. Bind the run to the exact AO2.7 M5 artifact: record its SHA, host label,
   TCP f8 p50, and calculated 80% floor.
2. Record Windows host facts, binary hashes, OS/architecture, benchmark-account
   identity, selected TCP f8 profile, active hook mode, and peer-wire mode.
3. Run the same safe lifecycle: account preflight, clean snapshot before roster,
   benchmark daemon start, roster/setup, timed TCP f8 profile, owned-daemon
   stop, exact restore, and post-restore read-only health proof.
4. Publish raw and compact artifacts with the M5 reference and the computed
   Windows threshold. Retain f1/f2 TCP diagnostics under identical metadata.

## Non-goals and boundaries

- No Windows-specific fast path, benchmark-only build, runtime flag, disabled
  hook, synthetic result, or threshold waiver.
- No modification to the Rust writer, Tokio/Axum router, TLS, client framing,
  or benchmark harness to chase a result during this evidence sprint.
- No comparison to an arbitrary historical Mac result; only the accepted AO2.7
  M5 artifact is authoritative.

If Windows is below the calculated floor, preserve the evidence and open a
separate, measured Windows performance plan. Do not change production code in
AO2.8.

## Acceptance criteria

| Requirement | Evidence |
| --- | --- |
| Comparable input | Same merged SHA, TCP f8 profile, hook/wire mode, and artifact schema as AO2.7. |
| Safety | Windows dedicated-account preflight and complete snapshot/restore proof. |
| Threshold | Windows TCP f8 p50 ≥ `0.80 × AO2.7 M5 TCP f8 p50`. |
| Integrity | All timed samples retained; accepted/durable counts and cleanup/restore pass. |
| Traceability | Raw evidence names both host labels, both exact SHAs, M5 p50, and computed floor. |
| Publication | Published via the AO2.9 finalizer on `evidence/ao2-benchmark-reports`, reachable through its reviewed Pages PR, with intent/result/index commits retained. |

Required gates are the existing validation suite at the tested SHA, the raw
physical Windows evidence, independent artifact review, and an explicit
comparison calculation. An unavailable fastpc4 leaves this sprint blocked;
M4/M5 results cannot substitute for Windows proof.
