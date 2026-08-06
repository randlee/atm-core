# AM.4 — Delete Legacy Peer Ingress and Egress

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.8, AM.1, and AM.2.
**unblocks:** AM.5 and AM.6.
**parallel_safe:** none; deletion changes active peer composition.

**traceability:** `REQ-CORE-TRANSPORT-001/002/004/006`,
`REQ-DAEMON-TRANSPORT-001/002/005/006`, ADR-033, ADR-040, ADR-041.

## Deliverables

1. Delete all peer-specific client/listener/decoder/router implementation,
   peer application body/header protocol, `PeerMessageArray` grammar, and
   associated fixtures/docs/dependencies listed by AM.1.
2. Retain only AL.7's TLS physical adapter around the shared AL.4 client and
   AL.2 handler. TLS provenance is authentication metadata, not peer request
   routing.
3. Enable the matching AM.1 guards in the deletion PR.

## Acceptance criteria

- M5 direct send has one active client/route/handler path and passes unchanged
  route-body/result snapshots.
- Search finds no peer-only DTO, decoder, header protocol, array grammar, or
  duplicate storage/nudge path.
- TLS/trust negative cases still fail before application dispatch.

## Required validation

- full test/format/lint suite
- M5 clean-checkout direct-send and mTLS-negative proof
- static negative guards and representative mutation proof

## Non-closure

This deletes peer transport divergence only; resend/replay deletion is AM.5.
