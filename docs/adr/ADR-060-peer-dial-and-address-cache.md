# ADR-060 — Peer Dial Order And Address Cache

| Field | Value |
| --- | --- |
| ID | ADR-060 |
| Status | Accepted (locked; change only by a superseding ADR) |
| Scope | Outbound cross-host peer dialing in `atm-http-runtime` (pooled mTLS connector and `plaintext-test` direct connector) |
| Relates to | ADR-040, ADR-047, `REQ-CORE-TRANSPORT-002D`, `REQ-CORE-TRANSPORT-002E`, `REQ-P-ADDRESS-001`, PR #1142 |

## Context

ADR-040 fixes *what* a peer is: one registered forward-resolvable hostname,
port, and certificate pin, with no durable IP alias and no reverse DNS. It
says nothing about how the resolved answer is dialed. The first cross-host
connect from a macOS host to `rand-m5.local` failed because macOS multicast
DNS answers with a routable IPv4 address and a scope-less link-local IPv6
address (`fe80::/10`, scope id 0). The kernel cannot route the scope-less
form, so a plain `TcpStream::connect(host)` failed outright or stalled for the
whole request budget, and the CLI saw a bare "could not connect" error.

The operator's stated expectations, recorded here as application
requirements rather than preferences:

- the target is `rand-m5`, `rand-m5.local`, or an exact IP; `.local` is an
  implementation detail and its presence or absence must never change the
  code path or the cache entry used;
- the shared key and trust pin are stored against one hostname, and the
  peer's IP changes as the machine moves between wired, Wi-Fi, and VPN;
- a short-term cache of at least five minutes exists, a valid entry is found
  in well under one millisecond, and an address change is followed inside the
  same request without an error reaching the caller; re-resolving on a stale
  entry may take milliseconds.

## Decision

All outbound peer dialing goes through one module,
`crates/atm-http-runtime/src/peer_dial.rs`. The rules below are the design of
record; they are not to be relaxed, reordered, or duplicated elsewhere.

1. **Resolution.** The registered hostname is forward-resolved through the
   operating-system resolver at connect time (`tokio::net::lookup_host`).
   No ATM-side resolver, no reverse lookup, no durable storage of the answer.

2. **Ordering and filtering.** From the resolver answer: IPv6 addresses that
   are link-local with scope id 0 are dropped as unroutable (a scoped
   `fe80::…%if` stays usable); IPv4 addresses come first, then the remaining
   IPv6 addresses; the list is capped at `MAX_DIAL_CANDIDATES = 4`. Dropped
   and truncated counts are carried into diagnostics.

3. **Bounded dial loop.** Candidates are dialed in order. Each attempt is
   bounded by `remaining_budget / untried_candidates`, recomputed per attempt,
   so an unresponsive first address leaves the later addresses their share.
   Every failed attempt is recorded as `address: reason`.

4. **Process-local address cache.** `PeerAddressCache` maps
   `(canonical host, port)` to the ordered candidate list with a TTL
   (`PeerPoolConfig::address_cache_ttl`, default five minutes, must be
   non-zero). The key is the ASCII-lowercased hostname with a trailing
   `.local` removed, so `rand-m5`, `rand-m5.local`, and `RAND-M5.local` share
   one entry. A hit is a mutex-guarded `HashMap` lookup and a small clone: no
   I/O, well under a millisecond.

5. **Stale-entry recovery.** A fresh cache entry is dialed with at most the
   smaller of half the remaining budget and `STALE_ADDRESS_DIAL_CAP`
   (500 ms). If that fails, the entry is forgotten, the name is resolved again
   (bounded by the remaining budget), the new answer is stored, and it is
   dialed with what remains. The caller sees an error only when the fresh
   answer also fails to connect, and that failure removes the entry so the
   next request resolves afresh.

6. **Both wire-security modes dial alike.** The pooled mTLS connector uses
   `PeerAddressCache::connect`. The `plaintext-test` direct `reqwest` client
   uses `OrderedPeerResolver` (`reqwest::dns::Resolve`) which applies rule 2
   and returns IPv4 first so hyper's connector prefers it; it layers no
   private cache because the OS resolver cache already answers in
   microseconds and `reqwest` pools established connections.

7. **Diagnostics.** Connect failures are logged at `warn` with the peer
   authority and the per-address cause. The public error message names the
   remote authority and the per-address causes (the HTTP boundary redacts the
   separate `cause` field, so the diagnosis must live in the message). The
   dial loop is bounded `DIAL_REPORT_GRACE` (250 ms) *inside* the request
   deadline, reserved out of the budget rather than added to it, because the
   caller's own request timeout fires at the full deadline; the loop therefore
   finishes first and its per-address diagnosis, not a bare timeout, is what
   the caller sees. When the remaining budget is at or below the grace the
   loop uses what remains. Message bodies, tokens, and raw configuration are never
   logged (#904).

## Timing contract

These are the non-functional bounds the implementation must meet; they are
derived from the 3 s `SERVER_REQUEST_BUDGET` (`atm-storage::request_budget`),
of which roughly 2.9 s is normally left for the peer write.

| Situation | Bound |
| --- | --- |
| Valid cache entry | under 1 ms to find; one mutex-guarded map lookup, no I/O |
| Cold cache, OS resolver cache hit | sub-millisecond |
| Cold cache, unicast DNS (LAN or VPN) | milliseconds to ~100 ms, acceptable |
| Cold cache, mDNS with retransmits after a network change | up to ~2 s, covered by the budget |
| Stale entry, old address black-holed | at most 500 ms spent, then at least ~2 s left for lookup + dial: covered |
| Resolver never answers, or peer asleep / off network | named failure inside the budget; no cache entry left |

No path exceeds the request budget; the dial loop finishes
`DIAL_REPORT_GRACE` before it so the per-address diagnosis is what surfaces.

## Boundary and enforcement

`peer_dial.rs` is the only module in the workspace that performs peer name
resolution (`lookup_host`, `ToSocketAddrs`, `reqwest` `dns_resolver`) or
dials a peer TCP stream from `atm-http-runtime`; the one other resolver use is
the CLI's literal-IP-to-trusted-host check owned by ADR-040. The
`peer-dial-seam` lint (`.just/lint_peer_dial_seam.py`, wired into `just lint`
and CI) fails when resolution or dialing appears elsewhere, when
`shared_direct_peer_client()` stops installing `OrderedPeerResolver`, or when
any locked constant, the arithmetic that applies it (the `min` with
`STALE_ADDRESS_DIAL_CAP`, the `DIAL_REPORT_GRACE` subtraction), or the
cache-key normalization drifts from this ADR. Dialing is matched as
`TcpStream::connect`, `TcpStream::connect_timeout`, and `TcpSocket::connect`.
The lint has its own unit tests under `.just/tests/`. The
`BOUNDARY-HttpRuntime` record names the seam under `io_owns`, forbids peer
resolution outside it, and lists the lint under enforcement. Changing the
locked design means: supersede this ADR, update `REQ-CORE-TRANSPORT-002E`,
then update the lint's expected lines in the same change.

## Consequences

- A macOS-to-macOS `.local` peer connects on the first attempt in both
  wire-security modes; a link-local-only answer fails fast with the reason
  named.
- A peer that changes address is reached inside one request after at most
  one wasted dial of at most 500 ms against the old address; no error
  escapes.
- Steady-state throughput is unaffected: the module runs only on connection
  establishment (independently confirmed by a hot-path review on PR #1142:
  pooled reuse, `reqwest` pooled reuse, UDS, and loopback paths are
  unchanged; a cache hit is one mutex-guarded map lookup, no I/O). Pooled connections and `reqwest`'s own pool bypass it
  entirely for requests on an existing connection. Same-host UDS and loopback
  paths do not use it.
- Any future change to ordering, cap, budget split, stale-dial cap, TTL
  floor, cache key normalization, or the dual-mode guarantee requires a
  superseding ADR and an update to `REQ-CORE-TRANSPORT-002E`; the
  `peer-dial-seam` lint fails until its locked lines are updated in the same
  change, and QA verifies implementations against this document.

## Verification

Unit coverage lives in `peer_dial.rs` tests: ordering and link-local
filtering, candidate cap, mixed-family connect without stalling, bounded
first attempt, link-local-only fast failure, per-attempt causes, cache reuse
within TTL, shared entry for bare and `.local` names, re-resolution on a
changed address without error, stale dial bounded by the cap, TTL expiry
(without sleeping), no entry left behind on failure, and
`OrderedPeerResolver` output shape. `peer_connection_pool.rs` proves the
per-address diagnosis surfaces instead of a bare timeout and that a zero TTL
is rejected.
