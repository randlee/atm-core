# ADR-040 — Peer Authority Resolution

| Field | Value |
| --- | --- |
| ID | ADR-040 |
| Status | Proposed |
| Scope | Repository-wide |
| Relates to | ADR-034, ADR-035, Phase AI.25, #901, #902 |

## Decision

A cross-host peer is registered by a stable hostname, HTTPS port, and
pinned certificate fingerprint. The durable record stores only that endpoint
identity and pin; it never stores a resolved IP alias.

The registered name is a forward-resolvable endpoint name. It may be an
ordinary DNS/DDNS name or a stable local-network mDNS name such as
`rand-m5.local`. The peer operator is responsible for keeping that name
resolvable when VPN, Wi-Fi, Ethernet, DHCP, or VPS placement changes. ATM
never discovers a peer name by reverse DNS and does not write an observed
address back into durable state.

The CLI reads the enabled authority set directly through
`PeerConfigStore::list_trusted_peers()` at the approved
`boundaries/atm-storage/peer-config-store.toml` seam. It does not invent a
parallel configuration read and it does not depend on a running daemon or the
daemon's separately refreshed in-memory TLS-verifier snapshot. A later send
still needs the ordinary local-daemon path, but address normalization itself is
available while that daemon is stopped.

## CLI address convenience

The CLI resolves destination shorthand before it builds the existing
fully-qualified HTTP request. The daemon, request schema, wire protocol,
storage schema, and graft/native-tool APIs receive no new shorthand and do no
peer-name completion.

- A trusted host input may omit a terminal `.local`: `rand-m5` and
  `rand-m5.local` select the same canonical enabled trusted authority. The
  canonical registered hostname is used in the request, output, message
  provenance, and diagnostics.
- `agent@host` is same-team cross-host shorthand. If `host` is not a known
  local team but resolves to exactly one enabled trusted authority, the CLI
  expands `agent@host` to `agent@<caller-team>.<canonical-host>`.
- `agent@team.host` remains the explicit different-team cross-host form. The
  host component receives the same `.local`-optional trusted-authority
  resolution.
- Existing `agent@team` handling remains compatible: an exact known team name
  wins over host shorthand. A string matching neither a known team nor one
  enabled trusted authority fails closed with structured, actionable recovery.
- Hostname comparisons are ASCII case-insensitive. Completion is deliberately
  limited to the terminal `.local` form; ATM does not apply fuzzy, prefix, or
  arbitrary suffix matching.
- `--host <host>` uses this exact same trusted-authority canonicalization as an
  inline host. Supplying both forms compares their canonical registered names,
  not their unnormalized spelling.

This is an input-normalization convenience only. The resolved `AgentAddress`
is indistinguishable from one supplied in fully-qualified form before the
existing HTTP request is constructed.

The CLI accepts either the registered hostname (including the documented
`.local` shorthand) or a literal IP as a destination input:

- after CLI normalization, a hostname input must exactly match one registered
  canonical hostname;
- a literal IP input is authorized only when a fresh bounded DNS/mDNS lookup of
  exactly one registered hostname contains that address;
- zero matches are untrusted and two or more matches are ambiguous; both fail
  closed before connection; and
- reverse-DNS lookup is forbidden. An IP-only registration never authorizes a
  hostname.

After authority resolution, the selected registered hostname and its pin are
the TLS authority. DNS/mDNS is routing discovery, not authorization;
certificate pin verification remains mandatory. The inbound peer supplies its
configured hostname and port as authenticated transport metadata, and the HTTPS
adapter verifies that endpoint identity plus its mTLS certificate pin before
constructing `AuthenticatedPeer`.

Changing a durable trust record must atomically refresh the daemon's live peer
authority snapshot through the control plane. It must not require a second
daemon or an operator restart merely to make a trust add/revoke effective.

## Consequences

Operators configure stable names such as `rand-m5.local` and
`fastpc4.radiant.local`; an address change does not require an SQLite migration
or a new alias record. A direct IP remains usable for diagnostics and
compatibility only while it resolves from one registered hostname. This decision
does not add DNS/mDNS results, retries, receipts, or delivery state to storage.

### Platform boundary

macOS, Linux, and Windows all receive the same canonical hostname, matching,
failure, and recovery behavior. Ordinary DNS/DDNS is the portable baseline.
An operator may register a `.local` name only where the operating system's
network stack has mDNS available; on Windows without mDNS resolution, ATM
fails before dispatch with the same structured unresolved-authority recovery
as an unavailable DNS name. The operator must use a forward-resolvable
DNS/DDNS authority or enable the supported local mDNS facility; ATM does not
silently fall back to an IP or another peer. Windows test coverage must cover
this fail-closed behavior and successful canonicalization independently of a
particular LAN's mDNS service.

A host with several account-owned daemons assigns each daemon a distinct
configured HTTPS port and a distinct endpoint name or otherwise unambiguous
authority record. They may resolve to the same current IP. A port bind collision
is an availability failure; ATM must not silently select another port. The
client's source IP is never peer identity, so a VPN address change does not
alter inbound authorization.
