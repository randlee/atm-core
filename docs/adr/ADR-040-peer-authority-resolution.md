# ADR-040 — Peer Authority Resolution

| Field | Value |
| --- | --- |
| ID | ADR-040 |
| Status | Superseded by ADR-047 |
| Scope | Repository-wide |
| Relates to | ADR-034, ADR-035, Phase AI.25 |

## Decision

A cross-host peer is registered by a canonical hostname, HTTPS port, and
pinned certificate fingerprint. Durable explicit aliases may map either a host
label/full hostname or an `IpAddr` literal to exactly one enabled canonical
peer. The durable message record stores only the canonical hostname; it never
stores a dynamically resolved IP address.

`PeerDirectory` is rebuilt from trusted-peer and alias configuration only at
daemon startup or authenticated reload. Admission parses a literal IP before a
hostname and makes one expected-O(1) directory lookup. It performs no DNS,
reverse lookup, peer scan, socket I/O, worker dispatch, or per-send SQLite
query. Canonical-host self aliases are synthesized in that snapshot, not stored
as duplicate alias rows.

After normalization, the selected canonical hostname and pin are the TLS
authority. Certificate pin verification remains mandatory. The inbound peer
supplies its configured hostname and port as authenticated transport metadata,
and the HTTPS adapter verifies that endpoint identity plus its mTLS certificate
pin before constructing `AuthenticatedPeer`.

Changing a durable trust record must atomically refresh the daemon's live peer
authority snapshot through the control plane. It must not require a second
daemon or an operator restart merely to make a trust add/revoke effective.

## Consequences

Operators configure stable names such as `fastpc4.rz.local`, plus an explicit
IP alias when an operator wants an IP input to select that name. An address
change is a configuration mutation, never DNS discovery at admission. This
decision does not add DNS results, retries, receipts, or delivery state to
storage.

A host with several account-owned daemons assigns each daemon a distinct
configured HTTPS port and a distinct endpoint name or otherwise unambiguous
authority record. They may resolve to the same current IP. A port bind collision
is an availability failure; ATM must not silently select another port. The
client's source IP is never peer identity, so a VPN address change does not
alter inbound authorization.
