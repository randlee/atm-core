---
phase: AO2
sprint: AO2.8
title: Windows full benchmark-matrix parity after accepted M5 suite
branch: future-evidence-worktree
integration_branch: integrate/phase-ao2
status: draft_for_review
depends_on:
  - AO2.7-m5-tcp-benchmark-parity
---

# AO2.8 — Windows full benchmark-matrix parity after accepted M5 suite

## Decision

AO2.8 runs the same mandatory `sqlite`, `uds`, `tcp`, and `tcp-tls` suite on
fastpc4/Windows after AO2.7 has produced an accepted M5 suite for the same
merged SHA. Windows is not permitted to run only TCP: if a required target is
not implemented or supported, the Windows suite is blocked and the missing
target must be fixed before parity can be claimed.

For each target, the Windows f8 median must be at least 80% of the matching
accepted M5 f8 median. The suite report calculates all four floors from the
referenced M5 artifact; no floor is hardcoded. This is a cross-hardware parity
floor, not a claim that the machines should have identical absolute results.

## Required procedure and closure

1. Bind the suite to the accepted M5 manifest: exact source SHA, binary hashes,
   target medians, and computed 80% floor for every target.
2. Run one ordinary `just benchmark` invocation under Windows' dedicated
   benchmark OS account. It must retain the complete four-target matrix,
   snapshot-before-roster, exact restore between targets/finally, and no
   interactive-account access.
3. Preserve all raw samples, percentiles, accepted/errors, target diagnostics,
   suite settings, daemon logs, and restore evidence.
4. If any Windows target misses its floor, take the first failing target in
   `sqlite`, `uds`, `tcp`, `tcp-tls` order; reproduce it, inspect the released
   hot path against its faster adjacent layer, remove the measured unjustified
   work while retaining functionality, add a focused regression test, run all
   correctness/boundary gates, and rerun the entire Windows matrix. An
   ordinary test, harness, configuration, or runtime failure is fixed in that
   iteration, not reported as a stopping point. Do not tune one target in
   isolation, skip UDS, disable TLS, or use a Windows-only fast path.
5. AO2.8 passes only when all four targets meet their computed floors in one
   complete suite. If no safe improvement remains after exhaustive profiling,
   it is blocked with the same complete analysis required by AO2.7, never
   passed as partial evidence.

## Acceptance criteria

| Requirement | Evidence |
| --- | --- |
| Complete suite | Exactly one accepted artifact each for `sqlite`, `uds`, `tcp`, and `tcp-tls`; no omitted target. |
| Comparable source | Same merged SHA and matched released CLI/Tokio-Axum daemon pair as M5. |
| Thresholds | Each Windows f8 median ≥80% of the matching accepted M5 f8 median. |
| Safety | Dedicated-account preflight and verified snapshot/restore before, between, and after targets. |
| Integrity | All target samples and diagnostics are retained; no unexpected errors or partial publication. |
| Closure | One complete all-target Windows suite passes, or the sprint remains blocked only by an evidenced major/external constraint after exhaustive measured remediation. |

M5, M4, synthetic results, or a TCP-only artifact cannot substitute for this
Windows result.
