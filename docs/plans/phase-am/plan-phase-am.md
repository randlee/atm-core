---
title: Phase AM Plan — Remove the Legacy Transport Stack
status: draft
branch: plan/tokio-migration
baseline: develop @ 67401907039f92e58e883273f02372a637202f70 plus accepted Phase AL
---

# Phase AM — Deletion-Only Transport Cleanup

## Goal

Delete everything made redundant by `atm-http-runtime`. AM is intentionally
deletion-only: it removes transport machinery; it does not preserve, repair,
or extend that machinery. The result is one simple, consistent typed HTTP path
maintained by standard libraries.

## Entry gate

AM implementation begins only after AL.5 proves the new runtime is the live
local and cross-host path. AM may perform inventory and write static guards in
parallel with AL, but it must not delete a live path before that proof.

The binding rules are in
[`phase-al-am-runtime-boundary-checklist.md`](../phase-al-am-runtime-boundary-checklist.md).

## Removal scope

Delete, rather than deprecate or wrap, the following categories once unused:

1. `HttpFrameReader` and all handwritten HTTP parsing, framing, request
   writing, response writing, and raw response decoding.
2. Legacy local UDS/loopback TCP transport workers whose responsibility is
   manually accepting connections, reading frames, or writing frames.
3. Peer-specific client/listener/decoder/router code, including peer-only
   request types, provenance body/header protocol, and parallel ingress.
4. Resend/replay machinery: schedulers, cache/state maps, drain/recovery
   coordinators, queues, worker threads, retry timers, and their tests.
5. Legacy transport-specific observability, capacity registries, and state
   machines with no consumer after AL.
6. Tests, fixtures, documentation, and Cargo dependencies that exist solely
   to support a removed implementation.

`atm-peer-tls-interop` and `atm-storage/src/tls.rs` remain quarantined
reference material unless a later explicit decision changes that status. AM
does not route production traffic through them and does not delete them.

## Sprints

### AM.1 — Removal ledger and negative boundary guards

**Depends on:** AL.1 for the target crate boundary; may run before AL.5.

- Create a reviewed removal ledger that names every legacy production module,
  its remaining callers, the AL replacement, and the deletion PR that owns it.
- Add architecture tests that fail on prohibited module names, raw framing
  symbols, direct SQLite imports, peer-only request types, and resend symbols.
- Add dependency-edge checks: no daemon/runtime reference to tmux/graft,
  SQLite/rusqlite, or legacy peer transport crates.

**Accept when:** every live legacy symbol has an owner or is proven dead; the
guards fail if a representative prohibited symbol is reintroduced.

### AM.2 — Delete legacy ingress and egress

**Depends on:** AL.5 and AM.1.

- Delete the handwritten local and peer transport modules and remove their
  module declarations, exports, construction paths, test fixtures, and
  dependencies.
- Delete duplicate peer request/response grammar and parallel decoder/router
  code. The canonical runtime handler remains the only ingress.
- Delete the hand-rolled client path. All callers use AL's shared typed client.

**Accept when:** repository search finds no active legacy framing/peer ingress
symbols, compilation has no deprecated compatibility allowance for them, and
local/cross-host smoke succeeds through AL.

### AM.3 — Delete recovery/replay complexity

**Depends on:** AM.2.

- Delete resend/replay schedulers, retry queues, drain coordinators, peer
  state maps, background workers, and related configuration/doctor surfaces.
- Delete obsolete tests rather than converting them into tests for a removed
  feature. Retain only direct-send and received-hook behavior tests.
- Verify there is no timer-triggered cross-host send path after removal.

**Accept when:** static guards prove no automatic resend/replay implementation
exists and a failed direct send returns its ordinary failure without spawning
background work.

### AM.4 — Minimality audit and phase proof

**Depends on:** AM.2 and AM.3.

- Audit each remaining daemon/runtime module against the composition-only
  boundary. Remove residual application transport policy or move it to the
  appropriate core trait implementation.
- Run full tests, formatting, lint, local smoke, M5 smoke, and the benchmark
  comparison initiated in AL.5.
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
