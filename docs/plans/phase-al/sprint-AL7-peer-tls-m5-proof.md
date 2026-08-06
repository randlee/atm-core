# AL.7 — Authenticated Peer TLS Adapter and M5 Proof

**recommended_agent:** arch-ctm/deep-reasoning; M5 team executes the remote
clean-checkout proof.
**must_follow:** AL.2, AL.4, and accepted existing TLS/trust policy.
**unblocks:** AL.8.
**parallel_safe:** AL.3, AL.5, and AL.6 after AL.4 is merged; this sprint owns
only TLS connection/authentication setup.

**traceability:** `REQ-CORE-TRANSPORT-001/002/002A/002B/002B1/002C/004/005`,
`REQ-DAEMON-TRANSPORT-001/003/005`, ADR-033, ADR-040 (proposed input only),
ADR-041.

## Deliverables

1. Configure the existing durable trust/authority view and Rustls mTLS
   connector/listener around the shared AL.4 client and AL.2 router. The
   adapter contributes authenticated peer provenance only after certificate
   and exact allowlist verification.
2. Use the existing hostname/port/pin policy without persisting observed IPs,
   adding peer delivery state, or treating socket address as identity.
3. Keep the explicit plaintext-test profile process-local and untrusted. It
   reaches the same handler only for smoke diagnosis; it cannot authorize a
   recipient or count as mTLS proof.
4. Provide the M5 team a pinned commit, clean-checkout command, fixture setup,
   expected unchanged JSON artifact, and machine-readable pass/fail artifact.

## Acceptance criteria

- One direct M5 cross-host write uses the exact existing `POST /messages`
  body/result and the same `ApiRouter`/storage/hook call path as local traffic.
- No peer listener/client/decoder/router, `PeerMessageArray`, peer header
  application protocol, retry, replay, queue, or coordinator is introduced.
- An untrusted/mismatched peer cannot reach application dispatch; a direct
  connection/DNS/TLS failure returns the normal typed direct-send outcome and
  starts no background work.

## Required validation

- mTLS allowlist positive and negative integration tests
- plaintext-test isolation test
- direct failure test with task-accounting assertion
- M5 clean-checkout cross-host proof, including artifact and SHA

## Non-closure

This is direct-send proof only. It creates no replay/reconciliation mechanism
and does not delete legacy peer code.
