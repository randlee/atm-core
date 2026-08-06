# AL.6 — Loopback TCP Adapter

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.2 and AL.4.
**unblocks:** AL.8.
**parallel_safe:** AL.3, AL.5, and AL.7 after AL.4 is merged; this sprint owns
only loopback physical setup.

**traceability:** `REQ-CORE-TRANSPORT-001/001B`,
`REQ-DAEMON-TRANSPORT-001`, `005`, `008`, ADR-033.

## Deliverables

1. Implement loopback-only listener/connect setup with the existing endpoint
   record and local capability authentication contract.
2. Attach it to the same AL.2 router and AL.4 client. The local capability is
   converted to `AuthenticatedIngress::Local` before the handler and never
   alters the route body/caller/destination.
3. Keep all platform conditional code inside this adapter. Composition,
   handler, storage, and hook code remain platform-neutral.

## Acceptance criteria

- Unix loopback and Windows-compatible test fixtures execute the same route
  and result contract as AL.5 UDS.
- The listener binds only loopback and rejects missing/stale/mismatched
  endpoint record or capability before `ApiRouter`.
- No loopback-only request type, wire codec, recipient routing, or nudge path
  exists.

## Required validation

- loopback integration smoke and capability-auth negative tests
- Unix UDS/loopback parity test using the same typed fixture and expected JSON
- Windows CI lane or verified equivalent platform fixture
- shutdown/drain and body-limit tests

## Non-closure

This sprint neither provides remote peer TLS nor changes public transport
types/serialization.
