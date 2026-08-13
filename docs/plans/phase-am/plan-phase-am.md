---
title: Phase AM Plan — Remove the Legacy Transport Stack
status: complete
branch: integrate/phase-am
worktree: /Users/randlee/Documents/github/atm-core-worktrees/integrate/phase-am
baseline: develop @ 67401907039f92e58e883273f02372a637202f70 plus accepted Phase AL
---

# Phase AM — Deletion-Only Transport Cleanup

## Goal

Delete everything made redundant by `atm-http-runtime`. AM is intentionally
deletion-only: it removes transport machinery; it does not preserve, repair,
or extend that machinery. The result is one simple, consistent typed HTTP path
maintained by standard libraries.

## Entry gate

AM implementation begins only after AL.9 proves the new runtime is the live
local and cross-host path. AM may perform inventory and write static guards in
parallel with AL, but it must not delete a live path before that proof.

The binding rules are in
[`phase-al-am-runtime-boundary-checklist.md`](../phase-al-am-runtime-boundary-checklist.md).
The exact manifest/doc/allowlist transition for each deletion is in
[`phase-al-am-boundary-transition.md`](../phase-al-am-boundary-transition.md)
and must occur in the same deletion PR as code removal.

## Removal scope

Delete, rather than deprecate or wrap, the following categories once unused:

1. `HttpFrameReader` and all handwritten HTTP parsing, framing, request
   writing, response writing, and raw response decoding.
2. Legacy local UDS/loopback TCP transport workers whose responsibility is
   manually accepting connections, reading frames, or writing frames.
3. Legacy peer-specific decoder/router code, peer-only request types,
   provenance body/header protocol, and parallel ingress. This excludes the
   retained canonical direct-peer HTTP client and listener owned by
   `atm-http-runtime`.
4. Resend/replay machinery: schedulers, cache/state maps, drain/recovery
   coordinators, queues, worker threads, retry timers, and their tests.
5. Legacy transport-specific observability, capacity registries, and state
   machines with no consumer after AL. AM.1 must classify each actual module
   as retained or owned by AM.5; AM.5 owns removal and guards for
   `peer_delivery_observability` and any ledger-confirmed obsolete peer
   capacity/state registry. It must not delete an active request registry.
6. Tests, fixtures, documentation, and Cargo dependencies that exist solely
   to support a removed implementation.

TLS-quarantine review: the `atm-peer-tls-interop` and `atm-storage/src/tls.rs`
paths exist as quarantined, reference-only AL artifacts -- they are physical-
address/trust candidates, not sender replay workers, and no TLS activation is
authorized by AM.1. AM.1 records the current TLS physical-adapter candidates
and their retain/remove disposition in its removal ledger; AM does not create
a production route through any historical reference material.

## Sprints

### AM.1 — Removal ledger and negative boundary guards

**Depends on:** AL.1's pushed integration commit for the target crate
boundary; merge it forward before each AM.1 inventory/fix round. AL.1 PR
merge is not required because AM.1 is non-production.

- Draft a reviewed removal ledger that names every legacy production module,
  its remaining callers, the AL replacement, and the deletion PR that owns it.
  Compute a call-graph topological deletion order. Sprint numbering is not
  ordering authority: AM.2–AM.5 consume only the order frozen against AL.9's
  accepted live-reference graph, and a compiled caller always precedes the
  symbol it calls in that order.
- Add architecture tests that fail on prohibited module names, raw framing
  symbols, direct SQLite imports, peer-only request types, and resend symbols.
- Add dependency-edge checks: no daemon/runtime reference to tmux/graft,
  SQLite/rusqlite, or legacy peer transport crates.

**Accept when:** every live legacy symbol has an owner or is proven dead; the
draft ledger distinguishes retain/delete candidates; the frozen ledger
lifecycle is explicit (AM.1 draft → AL.9 graph → AM.1 freeze → AM.2–AM.5
consume); and guards fail if a representative prohibited symbol is reintroduced.

### AM.2 — Delete shared raw HTTP framing

**Depends on:** AL.9 proof/ledger acceptance, AM.1's accepted frozen ledger,
and AM.3's applicable local-listener deletion.  AM.3's branch may be merged
forward before its PR merges; AM.2 must not wait on PR mechanics once that
predecessor code is available.  AM.2 owns the remaining caller migration:
replace `atm-daemon-client`'s retained non-write compatibility exchange and
`atm-http-runtime`'s core raw-request conversion with framework/typed HTTP
boundaries, then delete the `HttpFrameReader` callee.  No later or unnamed
owner is permitted for those edges.

- Migrate the retained `atm-daemon-client` bootstrap/non-write
  read/ack/admin compatibility exchange to the shared typed HTTP client while
  preserving its public compatibility contract; canonical writes remain on
  AL's async runtime client.
- Move the active HTTP handler's `HttpRequest` → `ApiRequest` conversion out
  of `atm-core` raw-framing helpers and into the typed runtime boundary.
- Delete `HttpFrameReader`, handwritten request/response framing helpers, and
  their core tests/exports only after those two caller migrations leave the
  inventory empty.

**Accept when:** repository search finds no active raw ATM HTTP parser/writer;
the retained non-write compatibility surface uses the typed client; the
runtime owns its typed request conversion; and local/cross-host smoke still
succeeds through AL.

### AM.3 — Delete legacy local ingress and egress

**Depends on:** AL.9 and AM.1 plus the frozen ledger's designated predecessor.
For the raw-framing edge, AM.3 is the predecessor of AM.2: delete or migrate
the applicable compiled local callers before AM.2 deletes `HttpFrameReader`.
All named predecessor deletion PRs must be merged before this PR begins; AM.3
and AM.4 ordering is the ledger's explicit topology, not their numerical
labels.

- Delete the superseded UDS and loopback client/listener workers, module
  declarations, fixtures, and dependencies.

**Accept when:** local traffic has one runtime path and local smoke passes on
supported operating systems.

### AM.4 — Delete peer ingress and egress

**Depends on:** AL.9 and AM.1 plus the frozen ledger's designated predecessor.
All named predecessor deletion PRs must be merged before this PR begins; AM.3
and AM.4 ordering is the ledger's explicit topology, not their numerical
labels. AL.7's TLS adapter was never implemented and TLS is quarantined outside
the MVP, so it is neither a retained dependency nor an AM.4 proof requirement.

- Delete only legacy peer DTO/header/body grammar, parallel ingress, and their
  fixtures/dependencies. Do not modify the live canonical direct-peer path:
  `crates/atm-http-runtime/src/client.rs` (`direct_peer_tcp_client` and
  `DirectPeerWriteClient`) or `crates/atm-http-runtime/src/lib.rs`
  (`DirectPeerTcpConfig` and the `HttpRuntimeBuilder` direct-peer listener).

**Accept when:** peer traffic has one canonical route and the M5 direct-send
lane passes through those retained canonical client/listener paths without
legacy peer-specific DTOs, headers, bodies, or parallel ingress.

### AM.5 — Delete recovery/replay complexity

**Depends on:** the frozen ledger's designated completed predecessors (at least
AM.3 and AM.4 when they own callers). Those deletion PRs must be merged before
this PR begins; no numeric ordering assumption overrides the topology.

- Delete resend/replay schedulers, retry queues, drain coordinators, peer
  state maps, background workers, and related configuration/doctor surfaces.
- Delete obsolete tests rather than converting them into tests for a removed
  feature. Retain only direct-send and received-hook behavior tests.
- Verify there is no timer-triggered cross-host send path after removal.

**Accept when:** static guards prove no automatic resend/replay implementation
exists and a failed direct send returns its ordinary failure without spawning
background work.

### AM.6 — Minimality audit and phase proof

**Depends on:** every frozen-ledger deletion owner (including AM.3, AM.4, and
AM.5) has merged. This is a PR-completion gate, not a merge-forward gate.

- Audit each remaining daemon/runtime module against the composition-only
  boundary. Remove residual application transport policy or move it to the
  appropriate core trait implementation.
- Run full tests, formatting, lint, local smoke, M5 smoke, and the benchmark
  comparison initiated in AL.9.
- Review every AM removal against the shared checklist and record source
  references for each final proof.

**Accept when:** the daemon is a small composition/lifecycle root, the runtime
is the sole HTTP server/client implementation, all shared checks pass, and no
compatibility transport remains.

## Non-goals

- Rebuilding the removed resend/replay feature in a new abstraction.
- Retaining a fallback server/client for safety.
- Adding a peer-specific optimization, queue, batch grammar, or worker.
- Changing the core message serialization contract.

## Phase completion gate

AM completes only when deletion is demonstrable: the legacy modules,
dependencies, symbols, and tests are absent; the negative boundary tests
protect against reintroduction; and the new runtime passes local and M5 proof.
