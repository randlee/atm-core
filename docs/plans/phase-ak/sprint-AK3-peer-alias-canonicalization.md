---
title: AK.3 Canonical peer alias persistence
status: complete
branch: feature/pak-s3-canonical-peer-aliases
worktree: ../atm-core-worktrees/feature/pak-s3-canonical-peer-aliases
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.2
parallel_safe: false
---

# AK.3 — canonical peer alias persistence

## Closure

Normalize configured peer aliases before persistence. This changes only the
stored destination representation; AK.3 adds no outbound delivery.

## Fixed contract

```rust
#[derive(Clone, PartialEq, Eq, Hash)]
enum PeerAliasKey {
    Host(HostName),
    Ip(std::net::IpAddr),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct PeerEndpoint {
    canonical_host: HostName,
    port: NonZeroU16,
}

struct PeerDirectory {
    by_alias: HashMap<PeerAliasKey, PeerEndpoint>,
}

impl PeerDirectory {
    fn normalize(&self, alias: &PeerAliasKey) -> Result<PeerEndpoint, AtmError>;
    fn endpoint_for_canonical_host(&self, host: &HostName) -> Option<PeerEndpoint>;
}

trait PeerConfigStore {
    fn peer_directory(&self) -> Result<PeerDirectory, AtmError>;
    fn list_peer_aliases(&self) -> Result<Vec<(PeerAliasKey, HostName)>, AtmError>;
    fn save_peer_alias(&self, alias: PeerAliasKey, canonical_host: HostName) -> Result<(), AtmError>;
    fn remove_peer_alias(&self, alias: &PeerAliasKey) -> Result<bool, AtmError>;
}

enum PeerAliasCommand {
    List,
    Add { alias: PeerAliasKey, canonical_host: HostName },
    Remove { alias: PeerAliasKey },
}
```

`PeerDirectory` is one immutable configuration snapshot, built at configuration
load/reload and passed to admission. Its expected-O(1) local substitution maps `m5`,
`rand-m5.local`, or an explicitly configured `IpAddr` alias to exactly one
canonical full hostname such as `rand-m5.local`. SQLite stores only
`PeerEndpoint::canonical_host` in `peerOutbound.host`, never a resolved IP.
The canonical host is also a self-alias. A configured endpoint is admitted
offline; no live DNS, peer scan, thread, process-wide mutable cache, or
database read occurs during normalization.

## Type and boundary inventory

| Item | AK.3 role |
| --- | --- |
| `PeerAliasKey` | New exact alias input: host label/full host or literal `IpAddr`; it prevents conflating an IP with `HostName`. |
| `PeerEndpoint` | New canonical destination: full hostname plus port. This is the only peer destination passed beyond admission. |
| `PeerDirectory` | New immutable expected-O(1) alias index. It is built at configuration load/reload and has no background refresh. |
| `PeerDirectory::{normalize, endpoint_for_canonical_host}` | `normalize` is the one admission-time alias lookup. `endpoint_for_canonical_host` is AK.5's bootstrap-only canonical-host lookup; it returns the configured port with the host or `None`. Neither can query SQLite or the network. |
| `PeerConfigStore::{peer_directory, list_peer_aliases, save_peer_alias, remove_peer_alias}` | Additive configuration boundary. It returns/saves configuration data only; it performs no DNS or delivery. |
| `PeerAliasCommand` | New CLI command model for `list`, `add`, and `remove`. It accepts a validated `PeerAliasKey`; it never accepts a resolved address or triggers delivery. |
| `TrustedPeer` | Existing canonical peer configuration. Its host/port source `PeerEndpoint`; no second peer model. |
| `peer_aliases` | New configuration table, not an outbox/retry/delivery table. |

No other AK.3 struct, enum, trait, executor, or cache is authorized without a
plan amendment.

## Deliverables

1. Add `PeerAliasKey`, `PeerEndpoint`, and immutable `PeerDirectory`; keep the
   type at the configuration/admission boundary, not in message, nudge,
   roster, or agent/session state.
2. Add `peer_aliases(alias_kind TEXT NOT NULL CHECK (alias_kind IN ('host',
   'ip')), alias_value TEXT NOT NULL, canonical_host TEXT NOT NULL REFERENCES
   peer_trusted_peers(host) ON DELETE CASCADE, UNIQUE(alias_kind,
   alias_value))`. Host aliases use the existing canonical `HostName` spelling;
   IP aliases use `IpAddr::to_string()`. Migration validates that every
   canonical host exists and is enabled. Canonical-host self-aliases are
   synthesized in the snapshot, not duplicated in SQLite.
3. Add `atm peer alias {list,add <host-or-ip> <canonical-host>,remove
   <host-or-ip>}` through `PeerAliasCommand`, explicit peer-alias CRUD, and
   tests. Parse a literal IP as `PeerAliasKey::Ip` before treating the input as
   `PeerAliasKey::Host`. Alias creation rejects an unknown/disabled canonical
   host, a duplicate normalized key, and `alias_kind='host'` when its
   `alias_value` parses as an `IpAddr`. It never calls DNS or discovers
   aliases.
4. Delete the legacy `runtime_health/peer_authority.rs::{resolve_peer_authority,
   MAX_LITERAL_IP_AUTHORITY_CANDIDATES}` and `peer_resolution.rs::resolve_peer_socket_addresses`
   modules, including their DNS/fan-out tests and `lib.rs`/runtime module declarations.
   `PeerDirectory::normalize` is their sole replacement for recipient
   canonicalization; it performs configured-alias substitution only. It must
   not retain a compatibility wrapper, scan peer rows, or resolve an address.
5. Build/swap one `PeerDirectory` only at daemon configuration load/reload;
   parse the optional recipient host as either `HostName` or literal `IpAddr`,
   construct one `PeerAliasKey`, and normalize every host-qualified recipient
   before its one canonical
   persistence, including `peerOutbound.host`. Do not add a second outbox,
   delivery table, or per-send lookup query.
6. Preserve the canonical full hostname through send, mailbox, ACK, and nudge
   data; preserve AK.2's no-delivery behavior.
7. Exclusively own the alias/configuration edits to
   `docs/requirements.md` (`REQ-CORE-TRANSPORT-002A` and `-002D`),
   `docs/adr/ADR-040-peer-authority-resolution.md`,
   `docs/adr/ADR-035-canonical-write-ingress-and-host-routing.md`,
   `docs/atm-daemon/{architecture,boundaries,http-api}.md`,
   `docs/atm/{architecture,requirements}.md`, CLI help, and
   `docs/peer-pair-smoke.md`. The edits must say explicit aliases are
   configuration, canonical hosts are durable routing selectors, and no
   discovery occurs at admission. AK.4 may reference their resulting
   `PeerEndpoint`, but must not edit these alias subclauses.

## Explicit prohibitions

- No DNS lookup, reverse lookup, full peer-row scan, socket, network I/O,
  retry, timer, task, worker, channel, or thread.
- No dynamic IP discovery or persistence of a resolved IP in `peerOutbound`.
- No state derived from agent/session/roster/nudge data.

## Required validation

- Source gate: the legacy `resolve_peer_authority`,
  `resolve_peer_socket_addresses`, `MAX_LITERAL_IP_AUTHORITY_CANDIDATES`, and
  their module declarations are absent; no compatibility wrapper or DNS path
  remains at admission.
- Unit: host and `IpAddr` aliases resolve O(1) to one full hostname without
  SQLite, live DNS, a peer scan, network I/O, or a new thread.
- Unit: duplicate/unknown/disabled aliases fail at configuration mutation;
  canonical self-aliases are synthesized, not stored twice.
- Integration: host-qualified recipient input persists only the canonical host
  in `peerOutbound`; configured unreachable peer remains undelivered.
- Integration: after the AK.2 intentional no-delivery admission, a real curl
  frame to the configured peer listener still reaches canonical persistence and
  emits exactly one ordinary nudge for loopback, same-IP, and cross-host input;
  no test may add a local-host ingress branch to make this pass.
- Migration: existing peer configuration opens with an empty alias table;
  aliases are cascade-deleted with their canonical trusted peer; an attempted
  alias mutation for a disabled or deleted peer returns a typed error.
- Smoke: run `just smoke localhost`, `just smoke local-ip`, and the current
  configured `crosshost-curl-tls` receiver lane in both M4→M5 and M5→M4
  directions, using isolated test homes/databases. The staged curl proof is
  required here because AK.3 deliberately has no production outbound sender;
  it must not use `UntrustedSmoke`. AK.4 is the first sprint that may claim a
  complete production send/read/ACK/nudge chain and replaces this lane with
  the production plain-HTTP proof.
- `just lint` and `just test` pass.

## Dependencies

Before every AK.3 development/fix round, merge AK.2 into AK.3. Start AK.3 as
soon as AK.2 is pushed; do not wait for QA. AK.3 PR completion waits for AK.2
merge. Push AK.3, then start AK.4 with AK.3→AK.4 merge-forward.
`must_follow` is required because AK.4 sends AK.3's persisted canonical host;
it is not parallel-safe because both change host-qualified admission/routing.
