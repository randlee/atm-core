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

This is an ordinary host-qualified peer send: no mock transport, direct
dispatcher call, or loopback-only route was used. The release test suite also
exercises the pinned mutual-TLS ingress route in
`https_transport::tests::exact_pinned_mtls_peer_reaches_the_shared_router`.
