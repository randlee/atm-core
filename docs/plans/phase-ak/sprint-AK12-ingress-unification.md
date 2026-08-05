---
title: AK.12 Ingress unification
status: proposed
branch: feature/pak-s12-ingress-unification
worktree: ../atm-core-worktrees/feature/pak-s12-ingress-unification
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.11 merged to integrate/phase-ak
merge_gate: AK.11 merge commit
parallel_safe: false
quality_findings: [AK-MANDATE-002-INGRESS-FORK]
---

# AK.12 — Ingress unification

## Closure

Closes mandate item #2 from
`docs/plans/phase-ak/phase-ak-mandate-compliance-fix-scope.md` (F3): a
cross-host frame and a local frame are currently decoded by two different
functions (`decode_peer_write_request` vs the ordinary `decode_request`),
even though AK.11 already unified the *send* side onto the canonical
singleton writer/reader. This sprint unifies the *receive* side to match.

Collapse `decode_peer_write_request` into the ordinary `decode_request` plus
a thin provenance layer. The only permitted peer-listener differences after
this sprint are:

1. bind address (the peer listener binds to the configured cross-host
   interface, the local listener does not),
2. `X-ATM-Peer-Source-Host` header extraction,
3. `AuthenticatedIngress::Peer` tagging on the resulting `ApiRequest`.

The writes-only restriction on peer ingress, and any peer-only body grammar,
must not live in a parallel decoder. If a write-only restriction is still
required, it must be expressed as a post-decode check on the ordinary
decoded `ApiRequest`, not as a different decode path.

## Explicit prohibitions

- No new decoder, no peer-only request struct, no peer-only serialization
  helper on the direct-write path.
- No re-introduction of `PeerMessageArray` decoding here — that grammar's
  disposition is AK.13's scope (F4), not this sprint's.
- No change to the AK.11 send path (`peer_delivery_client.rs`,
  `send_configured_peer_write`) — this sprint is ingress-only.
- Do not touch `atm-peer-tls-interop` or `atm-storage/src/tls.rs`
  (operator-directed quarantine; see F0 in the fix-scope doc — never a
  target for deletion or relocation in this fix scope).

## Deliverables

1. `decode_peer_write_request` is deleted; the peer listener calls
   `decode_request` (or the shared decode entry point AK.11 already uses on
   the send side) and applies only the three permitted differences above.
2. A focused test proves a cross-host frame and a local frame decode via the
   same function: assert byte-identical request bodies (apart from the
   provenance header) produce identical `ApiRequest` values regardless of
   which listener received them.
3. `curl` can POST the identical singleton body to either listener with the
   same result — record this as PR evidence (command + response), not just
   a unit test claim.
4. Boundary record (`boundaries/atm-daemon/peer-http-adapter.toml` or
   equivalent) updated to reflect the single shared decoder.

## Required validation

- `just lint`, `just test`.
- The cross-host/local-frame-parity test above.
- `just smoke localhost` and `just smoke local-ip`.
- PR evidence includes the curl parity demonstration.

## Dependencies

Starts after AK.11 merges to `integrate/phase-ak`. AK.13 (dead-grammar
removal) depends on this sprint landing first, since the peer decoder this
sprint touches is where `PeerMessageArray` decoding currently lives.
