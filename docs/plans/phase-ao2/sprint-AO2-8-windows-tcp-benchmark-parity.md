---
phase: AO2
sprint: AO2.8
title: M5 full-matrix performance remediation and Windows parity
branch: future-dev-worktree
integration_branch: integrate/phase-ao2
status: draft_for_review
depends_on:
  - AO2.7-m5-benchmark-harness-contract
dependency_relations:
  - prerequisite: AO2.7
    relation: must_follow
parallel_safe_with: []
---

# AO2.8 — M5 full-matrix performance remediation and Windows parity

## Decision

AO2.8 consumes the AO2.7 contract and actively restores the intended operating
point. It is not evidence-only: a low number, test failure, or harness/runtime
defect starts root-cause and repair work in the same iteration. It preserves
all functionality and fixes bad implementation choices rather than removing
TLS, bypassing the public Tokio/Axum daemon path, disabling hooks, changing
the workload, or compiling a special binary.

The four M5 `f8-v1` target expectations are: `sqlite` ~=45,000, `uds` ~=24,000,
`tcp` ~=22,500, and `tcp-tls` ~=22,500 messages/second. Historical UDS is
5–10% faster than TCP, so a 24k target preserves that relationship. A 5%
run-to-run tolerance makes the closure floors SQLite >=42,750, UDS >=22,800,
TCP >=21,375, and TCP+TLS >=21,375 msg/s. The 16k TCP/TLS result is a material
regression, not an acceptable redefinition of success. The targets apply to
the released production path, not separate best-effort measurements. Windows
uses fastpc4 only after M5 acceptance; its target is 85% of each matching M5
median, with the same explicit 5% run-to-run tolerance.

## Required M5 remediation loop

1. Run the ordinary `just benchmark` suite on M5. It must publish all four
   results through AO2.7's exact `f8-v1` manifest contract; partial artifacts
   are invalid.
2. Choose the first failing target in the fixed order `sqlite`, `uds`, `tcp`,
   `tcp-tls`. All four values remain in every subsequent report, but that
   target remains the active investigation until a later complete suite clears
   it.
3. Inspect the released hot path and its faster adjacent layer. Quantify
   writer batch/transaction/commit/fsync work; allocations/copies and
   serialization; router/middleware/hook work; locks/channels; TCP framing and
   connection behavior; logging; and TLS stream/handshake work. Capture a
   profiler/allocation trace when wall-clock measurements cannot distinguish
   the limiting operation.
4. Make the smallest justified production correction, with a focused behavior
   and performance-invariant regression test. Preserve ordering, savepoints,
   reply-after-commit durability, typed error/recovery context, active hook,
   public API, and crate boundaries. TLS work must remain outside plaintext
   TCP's steady-state path.
5. Run focused tests, architecture/boundary checks, `just lint`, and
   `just test`; then rerun the complete M5 matrix. A target-only rerun is
   diagnostic evidence, not acceptance evidence.

An ordinary test, configuration, fixture, report-schema, daemon, or runtime
failure is repaired in the active iteration. Its progress report records the
reproduction, measured root cause, patch, test evidence, and the complete
matrix; reporting the failure alone is not an outcome.

## Reproducibility and host-noise protocol

Acceptance requires the final three **contiguous entries** in the complete M5
attempt ledger for one candidate revision and `f8-v1` profile to pass. Every
complete attempt, including a below-floor but error-free attempt, is appended
before its result can be reported; missing sequence numbers, an unrecorded
attempt, or three passing entries separated by a failure are non-accepting.
Each attempt must be independently snapshot/restored and retain raw samples.
The accepted artifact is the versioned JSON at
`docs/plans/phase-ao2/artifacts/ao2-7-m5-suite-<candidate_revision>.json`;
it contains the typed `M5AttemptLedger`, all suite IDs, raw hashes/paths,
host/kernel/power facts, process/load/memory/disk telemetry captured
immediately before and after each suite, and all four target distributions.

Host contention or power state is not an explanation without that telemetry.
It is material only when either a non-benchmark process consumed at least 20%
CPU for at least ten timed seconds, or one-minute load average exceeded 125%
of the logical CPU count. One materiality-confirmed remediation authorizes one
replacement three-suite series per candidate revision; a second requires the
checkpoint below. Stop only the owned benchmark daemon, eliminate the proven
competing process or restore the documented fixed-power condition, and append
the new series. The old run remains in the ledger; a quiet rerun cannot replace
it silently.

`accepted_m5` means exactly: the ledger schema validates,
`candidate_revision` equals the post-merge `integrate/phase-ao2` SHA,
harness/profile/raw hashes match, the final three ledger entries are contiguous
complete results, every target is error-free, every target meets its threshold,
and `M5AttemptLedger::derive_accepted_m5` recomputes `true`. The loader rejects
a serialized `accepted_m5` that disagrees with that calculation. The AO2.8
Windows phase fails closed if this artifact is missing, malformed, mismatched,
or non-accepted.

## Checkpoint and escalation

After three full root-cause/fix/retest cycles or two focused engineering days,
hold an explicit M5 checkpoint. This is **not** permission to stop: it packages
the full ledger, profiles/traces, rejected hypotheses, exact changed paths, and
the next highest-value fix. The work continues in an immediately created
continuation worktree unless an architecture/product decision says otherwise.

The 5% tolerance is already incorporated into every stated closure floor.
There is no additional allowance below a floor. A target below its floor, a
missing target, or an uninvestigated ordinary defect remains blocked and is
never passed without an explicit product decision that changes the documented
baseline and threshold.

## Windows parity phase

After `accepted_m5`, run the same three complete `f8-v1` suites under fastpc4's
dedicated benchmark account. For each target, compute the expected Windows
median as `accepted_m5_p50 * 0.85`; its closure floor is that expected value
times `0.95` (an effective 80.75% of the M5 value). Display both values rounded
half-up to two fractional messages/second, while comparing unrounded measured
values to the unrounded floor. The Windows artifact records both values and the
M5 manifest SHA.

Windows must execute `sqlite`, `uds`, `tcp`, and `tcp-tls`; a Windows
TCP-only/WSL/VM substitute is incomplete. It records a typed, validated
`WindowsHostFacts` object (native OS/CPU, power plan, Defender/AV state,
explicit absence of exclusions, virtualization/WSL state, and standard-token
status) rather than prose assertions. Its committed result is exactly
`docs/plans/phase-ao2/artifacts/ao2-8-fastpc4-suite-<candidate_revision>.json`,
which contains `WindowsParityArtifact`, its M5-ledger SHA, the frozen F8
profile, facts, and all complete suite attempts. It may not
elevate the benchmark account, change power policy, add exclusions, use WSL,
or use a Windows-only fast path merely to improve a result. Below-floor results
follow the same root-cause/fix/full-matrix loop before any conclusion. The
three-cycles/two-days checkpoint applies independently to the Windows loop:
it retains every attempt and starts a continuation rather than silently ending
the Windows phase.

## Acceptance criteria

| Property | Required proof |
| --- | --- |
| M5 closure | Three consecutive complete `f8-v1` M5 suites meet SQLite >=42,750, UDS >=22,800, TCP >=21,375, and TCP+TLS >=21,375 msg/s (the 5%-tolerant floors for 45k/24k/22.5k/22.5k historical parity targets). |
| No false completion | Low numbers and ordinary faults have RCA, repair, test/gate evidence, and a next full-matrix result. |
| M5 handoff | AO2.7's immutable, schema-valid `accepted_m5` manifest is committed at the fixed path for the exact tested post-merge SHA. |
| Windows parity | Three complete native fastpc4 matrices meet the 85%-of-M5 target with its stated 5% tolerance (>=80.75% of matching M5 values), using the stated precision rule. |
| Matrix reports | Each iteration reports suite ID/SHA, all four medians and thresholds, accepted/errors, active target, and change since prior suite. |
| Safety | Every suite uses AO2.5.4's dedicated account and verified snapshot/restore; no interactive database is accessed. |

## Rollback

Each remediation is a scoped production commit with focused regression tests;
revert the offending commit if a complete matrix exposes a regression. Harness
and benchmark-account safety remain intact. No result authorizes a legacy
synchronous daemon path or a bypass of the released CLI/daemon pair.

## Current operator procedure

For any new benchmark run, follow the canonical
[`benchmark-run` skill](../../../.claude/skills/benchmark-run/SKILL.md).
