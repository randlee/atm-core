# AM.2 — Delete Legacy HTTP and Peer Ingress

**recommended_agent:** arch-ctm/deep-reasoning
**must_follow:** AL.5 and AM.1.
**unblocks:** AM.4.
**parallel_safe:** none; deletion touches shared transport composition.

## Paths/categories to delete

- `HttpFrameReader` and ATM-owned HTTP header/body/frame parsing or writing.
- Legacy local UDS/TCP workers whose role is manual HTTP connection handling.
- Peer-only clients/listeners/decoders/routers and their wire grammar,
  including `PeerMessageArray` if still present.
- Their test fixtures, exports, documentation, and direct Cargo dependencies.

The exact ledger, not this list, is authoritative for paths at implementation
time; it must be updated from the accepted AL.5 reference graph.

## Acceptance criteria

- No production caller reaches a legacy listener/client/decoder.
- Repository guards find no active raw ATM HTTP parser/writer or peer ingress.
- Every client uses AL's shared typed client and every listener uses AL's one
  typed handler.
- Local and M5 smoke pass after deletion.

## Required validation

- compilation/test/format/lint suite
- negative-symbol architecture guards
- local UDS/loopback and M5 cross-host smoke

## Non-closure

Recovery/replay is deleted in AM.3, not silently retained in AM.2.
