---
title: AO.4 — Peer-Wire Operational and Performance Proof
status: planned
branch: feature/pao-s4-peer-wire-proof
target: integrate/phase-ao2
worktree: ../atm-core-worktrees/feature/pao-s4-peer-wire-proof
external_blockers: []
---

# AO.4 — Peer-Wire Operational and Performance Proof

**recommended_agent:** arch-ctm/deep-reasoning.
**must_follow:** AO.3 PR merged. AO.4 uses its immutable integration artifact;
merge the integration tip before every evidence/fix round.
**parallel_safe:** none. It certifies the assembled public daemon behavior and
its performance baseline.
**unblocks:** Phase AO closure.

**traceability:** ADR-034, ADR-035, ADR-041, ADR-047;
`REQ-CORE-TRANSPORT-002B1`, `-002C`; the benchmark and smoke procedures.

## Goal

Prove the shipped daemon's plain and mTLS modes function correctly and that
the layered addition did not regress the preserved plaintext pipeline.

## Scope Summary

Public benchmark/smoke controls, reproducible evidence, compatible-baseline
analysis, and physical-host proof only. No runtime or TLS implementation edits.

## Governing Requirements

`REQ-CORE-TRANSPORT-002B1` and `-002C`, plus the AO.1 requirement
reconciliation and benchmark evidence rules.

## Governing ADRs

ADR-034, ADR-035, ADR-041, and ADR-047.

## Governing Boundaries

The completed AO.2 `peer-tls`, AO.3 HTTP runtime/bootstrap manifests, and
benchmark/smoke runner boundary guards.

## Prerequisites

AO.3 is merged to `integrate/phase-ao2`; evidence uses that immutable release
candidate and the normal daemon-switch procedure.

## Hard Dependencies

AO.3 PR merged. Host availability is not a dependency for code closure, but
it is a hard dependency for each claimed physical-host proof.

## Deliverables

1. Extend `just benchmark` with public `tcp` (`plaintext-test`) and `tcp-tls`
   (mTLS) targets. Both start the same shipped daemon and select only the
   ordinary `--peer-wire-security` argument; neither rebuilds nor enables a
   Cargo feature.
2. Record benchmark provenance: commit, binary version, mode, host/OS/arch,
   frames per connection, hook mode, sample count, command, and exact baseline
   revision/profile. Compare plaintext only with a compatible plaintext
   baseline; do not substitute a low absolute threshold.
3. Run local, M4, M5, and FastPC4 plaintext and mTLS campaigns when the host
   is available. Record an unavailable host as a bounded blocked artifact, not
   as a successful physical-host result.
4. Prove bidirectional M4↔M5 send, read, `--requires-ack`, and reply in both
   modes through the ordinary daemon/CLI pair. Include positive mTLS delivery
   and negative pre-router authentication evidence.
5. Publish safe indexed reports; they may contain public fingerprints and
   commands but never private keys, certificate bundles, capability tokens, or
   raw trust data.

## Acceptance Criteria

- Plaintext throughput meets its compatible pre-AO baseline or a demonstrated
  material regression is root-caused and fixed before phase closure.
- mTLS results have an independent same-mode baseline after the first accepted
  campaign; they are not used to disguise plaintext regression.
- Every report identifies the exact daemon binary/mode and does not rely on a
  benchmark-only transport implementation.
- Functional success never substitutes for the required performance comparison
  or unavailable-host record.

## Required Validation

- `just benchmark --target tcp`
- `just benchmark --target tcp-tls`
- `just benchmark-report --rebuild`
- `just reports-index --check`
- `just smoke crosshost-send`
- `just test`

## Required Document Updates

- Benchmark/smoke procedures, report schema/index, AO evidence ledger, and
  release notes only after the compatible plaintext baseline passes.

## Split Recommendation

Do not split: functional proof and profile-compatible performance comparison
must be judged together so an absolute-rate pass cannot mask a regression.

## Error Inventory

| Failure mode | Stable code ownership | Required recovery |
| --- | --- | --- |
| Requested benchmark mode cannot start | Preserve the AO.1/AO.2 typed launch or configuration code in the report. | Correct launch/configuration and rerun; never substitute another mode. |
| Baseline is missing or profile-incompatible | AO.4 records a documented benchmark-evidence failure code/status. | Collect a compatible same-host/mode/profile baseline; do not claim a pass. |
| Physical host unavailable | AO.4 records a bounded blocked-evidence status, not a success code. | Restore host access and rerun the omitted physical proof. |
| Material plaintext regression | AO.4 records a blocking performance finding linked to exact samples. | Root-cause and fix before phase closure; do not waive it with mTLS results. |

## Paths To Delete

None. AO.4 must not delete the historical compatible-baseline artifacts.

## Non-Goals

AO.4 does not change transport implementation, relax trust policy, add a
fallback, or make an unavailable physical host look tested.

## Risks And Watchouts

Previous benchmarks mixed host, hook mode, transport, and frames-per-
connection. AO.4 must surface incomparable evidence rather than averaging it,
and must treat a plaintext regression as blocking even when mTLS is functional.
