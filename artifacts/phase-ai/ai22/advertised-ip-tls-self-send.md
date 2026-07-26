# AI.22 advertised-IP HTTPS self-send proof

Run on 2026-07-26 against the selected `1.3.2-beta.22` CLI and singleton
daemon. The peer interface was enabled on `192.168.128.82:43101` with one
configured HTTPS interface and a pinned trusted-peer entry for that address.

| Check | Result |
| --- | --- |
| `atm doctor --json` | healthy; daemon readiness `ready`; one peer interface enabled |
| `atm send arch-ctm@atm-dev.192.168.128.82 ... --json` | `sent` |
| Message ID | `01KYE6DWS2YVTDX0AFZ115QF39` |
| Live recipient nudge | received for the same message ID |
| `atm read --message-id` | returned the same ULID and payload |
| Peer delivery log | `write_persisted` followed by `peer_delivery_confirmed` for the same ULID |

## Canonical-path component evidence

The proof exercised the peer TCP listener in
`https_transport::handle_peer_connection`, which decoded the shared HTTP write
resource and invoked `ApiRouter::route(..., AuthenticatedIngress::Peer, ...)`.
The only production router implementation is `DaemonRequestDispatcher`; its
`route_write` method invoked the one `MessageWriter::write` implementation and
then `PostWriteRouter::dispatch`. The persisted row was readable before the
recipient's nudge for the ULID above. The architecture gate
`ai23_ingress_adapters_cannot_own_write_side_effects` prevents either HTTP
adapter from adding a separate persistence, ACK, or nudge implementation.

This is an ordinary host-qualified peer send: no mock transport, direct
dispatcher call, or loopback-only route was used. The release test suite also
exercises the pinned mutual-TLS ingress route in
`https_transport::tests::exact_pinned_mtls_peer_reaches_the_shared_router`.

**Independence note:** this artifact was authored in the same commit
(`95dbb094`) as the fix it evidences, so it does not by itself satisfy the
sprint doc's requirement for a genuinely independent quality-review
execution (tracked as AI23-BLOCK-003). See
`advertised-ip-tls-self-send-independent-review.md` in this directory for the
independently-executed proof that closes that gap.
