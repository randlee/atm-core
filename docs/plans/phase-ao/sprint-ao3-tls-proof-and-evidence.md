---
title: AO.3 — Prove optional mTLS activation and retain release evidence
status: planned
recommended_agent: Cipher-311d
---

# AO.3 — Prove optional mTLS activation and retain release evidence

## Scope

Prove the AO.2 enabled module on the canonical Tokio/Axum path with both
automated negative coverage and a controlled two-host run. Retain reports
under `site/reports/` and add them to the report index using the smoke-test
convention.

## Dependencies

- **must_follow:** AO.2's PR must merge before AO.3 begins, because this
  sprint certifies the exact integrated artifact rather than a moving branch.
- **parallel_safe:** none. The physical report and final release claim need
  the merged AO.2 artifact and its final configuration surface.
- **unblocks:** Phase AO closure and a future TLS-enabled deployment decision.

## Deliverables

1. Reproducible automated evidence that mTLS traffic reaches the canonical
   router/write/post-receive path and rejected TLS cannot dispatch.
2. A two-host mTLS smoke report that records only safe metadata: exact commit,
   artifact version, OS/architecture, registered hostnames, public certificate
   fingerprints, test commands, result, and report paths.
3. Positive bidirectional send/read/requires-ack/reply evidence and negative
   wrong certificate, wrong hostname/SNI, disabled peer, and plaintext-on-TLS
   interface evidence.
4. Regression evidence for local UDS, loopback TCP, and explicit plaintext
   peer diagnostic mode showing the TLS option did not create a second
   application path. The report distinguishes default-mTLS-after-exchange from
   deliberately disabled TLS benchmark/debug runs.

## Acceptance criteria

- Every positive mTLS operation proves the same router/handler evidence as the
  canonical plaintext route.
- Every negative case fails before application dispatch and reports a safe,
  typed error; no automatic plaintext retry occurs.
- Successful key exchange/provisioning demonstrably chooses mTLS by default;
  plaintext evidence is accepted only when the report records the explicit
  test/benchmark/debug override.
- Reports are indexed and contain no private key material, raw certificate
  bundle, token, or unnecessary network secrets.
- The final source audit shows the active runtime only selects/delegates to
  `atm-peer-tls`; Rustls, certificate behavior, TLS storage calls, and
  TLS-side business logic remain inside that crate.

## Required validation

- AO.1/AO.2 focused tests plus `just lint` and `just test` on the frozen
  candidate SHA.
- Deterministic integration matrix for all AO.3 positive and negative cases.
- Physical two-host run using the smoke-test skill, with report panel and
  master-index verification under `site/reports/`.
- Architecture/dependency guard checks for no frozen-daemon use, no fixture
  dependency, no TLS-to-plaintext fallback, one application router, and no
  TLS business/storage implementation outside `atm-peer-tls`.

## Non-closure

AO.3 cannot prove that a corporate firewall permits inbound connectivity.
Phase AP owns the prior, real-host reachability proof for that scenario.
