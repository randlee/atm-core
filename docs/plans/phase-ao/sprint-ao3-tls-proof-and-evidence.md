---
title: AO.3 — Prove peer-tls on the canonical ATM path
status: planned
recommended_agent: Cipher-311d
---

# AO.3 — Prove peer-tls on the canonical ATM path

## Scope

Certify the merged AO.2 artifact with automated negative coverage and a
controlled two-host run. The proof verifies that TLS wraps the existing path;
it does not introduce any new message or daemon behavior.

## Dependencies

- **must_follow:** AO.2 PR merged, because proof uses its immutable merged
  artifact and final runtime configuration.
- **parallel_safe:** none. This sprint owns the single final release claim.
- **unblocks:** Phase AO closure.

## Deliverables

1. Automated evidence for valid mTLS and rejected wrong certificate,
   wrong hostname, wrong pin, disabled peer, and plaintext-to-TLS cases.
2. A two-host mTLS smoke run proving bidirectional send/read/requires-ack/reply
   through the unchanged canonical handler.
3. Explicit plaintext diagnostic-mode regression evidence for local UDS and
   loopback TCP; it must show the override, not an automatic fallback.
4. Indexed safe reports under `site/reports/` containing candidate SHA,
   version, OS/architecture, registered hostnames, public fingerprints,
   commands, and results—never keys or raw certificate bundles.
5. Final source audit that `peer-tls` contains the TLS mechanics and the
   active runtime contains only selection/delegation/wrapping.

## Acceptance criteria

- Positive mTLS proves the same request/result semantics as the existing
  plaintext HTTP path.
- Every negative TLS case fails before application dispatch and does not retry
  via plaintext.
- The run proves mTLS is the default after successful existing key exchange;
  plaintext is accepted only with the explicit diagnostic override.
- The master reports index links the smoke and benchmark artifacts.
- Reports do not disclose private material.

## Required validation

- AO.1/AO.2 focused tests, `just lint`, and `just test` on the frozen
  candidate SHA.
- Deterministic positive/negative mTLS integration matrix.
- Two-host smoke using `/smoke-test`, report panel, and master-index check.
- Architecture/dependency guards for one handler path, no TLS downgrade, and
  no frozen-daemon or fixture runtime use.

## Non-closure

AO.3 does not prove corporate-firewall reachability. Phase AP owns its
separate real-host outbound-connectivity proof.
