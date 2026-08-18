---
title: AO.3 — Prove peer-tls on the canonical ATM path
status: in_review
recommended_agent: arch-ctm
branch: feature/pao-s4-tls-benchmark-modes
worktree: ../atm-core-worktrees/feature/pao-s4-tls-benchmark-modes
---

# AO.3 — Prove peer-tls on the canonical ATM path

## Scope

Certify the merged AO.2 artifact with automated positive/negative coverage and
local controlled smoke. The proof verifies that mTLS supplies an authenticated
stream to the existing path; it adds no message or daemon behavior. Retained
multi-host proof and performance evidence are owned by AO.5 so AO.3 is not
held open by M5 or FastPC4 availability.

## Dependencies

- **must_follow:** AO.2 PR merged; proof uses its immutable merged artifact.
- **parallel_safe:** none. This sprint owns the final Phase AO release claim.
- **unblocks:** Phase AO closure.

## Deliverables

1. Automated evidence for valid mTLS and rejected wrong certificate, wrong
   hostname, wrong pin, disabled peer, and plaintext-to-mTLS cases.
2. Local mTLS smoke and a machine-independent two-host runbook/harness proving
   the exact bidirectional send/read/requires-ack/reply checks that AO.5 must
   execute through the unchanged canonical handler.
3. Explicit plaintext diagnostic-mode regression evidence for local UDS and
   loopback TCP, showing the override rather than an automatic fallback.
4. Indexed safe reports under `site/reports/` containing candidate SHA,
   version, OS/architecture, registered hostnames, public fingerprints,
   commands, and results—never private keys or raw certificate bundles.
5. Final architecture audit proving `peer-tls` owns TLS configuration/streams;
   `atm_storage::tls` owns verification primitives; the runtime has only the
   opaque adapter calls.

## Acceptance criteria

- Positive mTLS preserves the existing plaintext request/result semantics.
- Every negative case fails before application dispatch and does not retry via
  plaintext.
- Local proof shows mTLS is default after exchange; plaintext is accepted only
  under the explicit diagnostic override.
- The master reports index links the smoke and benchmark artifacts without
  leaking private material.

## Required validation

- AO.1/AO.2 focused tests, `just lint`, and `just test` at the candidate SHA.
- Deterministic positive/negative mTLS integration matrix.
- Local `/smoke-test` run, XHTML panel, report-index check, and a reviewed
  two-host runbook that AO.5 can execute unchanged.
- Boundary checks for the sole adapter implementation, no fixture/legacy edge,
  and no concrete TLS/storage dependency in `atm-http-runtime`.

## Non-closure

AO.3 does not retain M4↔M5 or FastPC4 physical proof/benchmark evidence; AO.5
owns those hardware-dependent runs. Phase AP owns the separate real-host
corporate-firewall outbound-connectivity proof.
