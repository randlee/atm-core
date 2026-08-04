# ADR-040 — Peer Authority Resolution

| Field | Value |
| --- | --- |
| ID | ADR-040 |
| Status | Superseded by ADR-047 |
| Scope | Repository-wide |
| Relates to | ADR-034, ADR-035, Phase AI.25 |

## Decision

A cross-host peer is registered by a stable DNS hostname, HTTPS port, and
pinned certificate fingerprint. The durable record stores only that endpoint
identity and pin; it never stores a resolved IP alias.

The registered name is a forward-resolvable endpoint name: the peer operator
is responsible for keeping its A/AAAA record current through ordinary DNS or
DDNS when VPN or Wi-Fi addressing changes. ATM never discovers that name by
reverse DNS and does not write an observed address back into durable state.

The peer transport accepts either the registered hostname or a literal IP as a
destination input:

- a hostname input must exactly match one registered hostname;
- a literal IP input is authorized only when a fresh bounded DNS lookup of
  exactly one registered hostname contains that address;
- zero matches are untrusted and two or more matches are ambiguous; both fail
  closed before connection; and
- reverse-DNS lookup is forbidden. An IP-only registration never authorizes a
  hostname.

After authority resolution, the selected registered hostname and its pin are
the TLS authority. DNS is routing discovery, not authorization; certificate
pin verification remains mandatory. The inbound peer supplies its configured
hostname and port as authenticated transport metadata, and the HTTPS adapter
verifies that endpoint identity plus its mTLS certificate pin before constructing
`AuthenticatedPeer`.

Changing a durable trust record must atomically refresh the daemon's live peer
authority snapshot through the control plane. It must not require a second
daemon or an operator restart merely to make a trust add/revoke effective.

## Consequences

Operators configure stable names such as `fastpc4.rz.local`; an address change
does not require an SQLite migration or a new alias record. A direct IP remains
usable for diagnostics and compatibility only while it resolves from one
registered hostname. This decision does not add DNS results, retries, receipts,
or delivery state to storage.

A host with several account-owned daemons assigns each daemon a distinct
configured HTTPS port and a distinct endpoint name or otherwise unambiguous
authority record. They may resolve to the same current IP. A port bind collision
is an availability failure; ATM must not silently select another port. The
client's source IP is never peer identity, so a VPN address change does not
alter inbound authorization.
