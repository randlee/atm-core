---
title: Phase AL Plan — `atm-http-runtime`
status: draft
branch: plan/tokio-migration
baseline: develop @ 67401907039f92e58e883273f02372a637202f70
---

# Phase AL — Build the Minimal Tokio HTTP Runtime

## Goal

Replace ATM's hand-written synchronous HTTP framing and transport-specific
request processing with one small `atm-http-runtime` library. It uses Tokio and
maintained HTTP/TLS libraries to provide the same typed application contract to
all clients and all listeners.

AL is an additive replacement phase. It does not preserve the legacy transport
as a compatibility architecture and does not add resend/replay. Phase AM
deletes the legacy implementation once AL proves the replacement.

## Baseline and entry gate

- Implementation starts from the current `develop` baseline recorded above,
  which includes the completed Phase AJ merge.
- Before AL.1 implementation, merge or narrowly transplant the **already
  implemented** AK.11 `MessageReceivedHookEmitter` rename into the AL
  integration line. Do not create another hook trait or retain
  `PostSendHookEmitter` as an active interface.
- The exact guardrails are in
  [`phase-al-am-runtime-boundary-checklist.md`](../phase-al-am-runtime-boundary-checklist.md).
  Every AL PR must pass them before merging forward.

## Architecture

`atm-http-runtime` is a library, not a second daemon executable.

```text
atm / atm-graft / cross-host sender
    └── shared typed HTTP client
            └── RequestEnvelope / ResponseEnvelope
                    └── Tokio HTTP listener(s)
                            └── one typed /v1/atm/messages handler
                                    └── ApiRouter + injected core boundaries
                                            └── storage trait
                                            └── MessageReceivedHookEmitter

atm-daemon = composition, listener selection, lifecycle only
```

### Library choices

- **Tokio** owns asynchronous execution, listener lifecycle, cancellation, and
  bounded task execution.
- **Axum/Hyper** provide routing, JSON extraction, response construction, and
  HTTP protocol handling. ATM must not parse or frame HTTP itself.
- **Rustls** provides the authenticated TLS configuration for physical peer
  links. TLS identity contributes authenticated ingress provenance; it does not
  select a separate application route.
- A maintained Tokio HTTP client is used by both local adapters and
  cross-host sends. Connector setup may differ for UDS, loopback TCP, and
  TLS TCP; body serialization, endpoint path, request dispatch, response
  decoding, and outcome handling do not.

These choices use the maintained Tokio-compatible server/router and protocol
libraries rather than reinventing HTTP. Axum is explicitly designed to run on
Tokio/Hyper and provides typed routing and request extraction; Hyper provides
the async HTTP implementation. [Axum documentation](https://docs.rs/axum/latest/axum/)
and [Hyper documentation](https://docs.rs/hyper/latest/hyper/) are the
implementation references; versions are selected and lockfile-pinned during
AL.1.

### Fixed boundaries

- The public wire body remains the existing shared `RequestEnvelope`; results
  remain the existing shared `ResponseEnvelope`. `WriteRequest` is never
  wrapped in a peer-only type or array grammar.
- `POST /v1/atm/messages` invokes one typed handler. Local and peer calls
  differ only in connector setup and trusted ingress provenance. The handler
  dispatches through the existing core `ApiRouter`, not a runtime-private
  decoder or dispatcher.
- The daemon and runtime use the existing sealed core storage boundaries; they
  never reference SQLite or a `rusqlite` implementation.
- After a newly persisted inbound message only, the shared dispatch path calls
  the injected `MessageReceivedHookEmitter`. A hook failure produces retained
  warning data but cannot turn a successful persistence/receive into failure.
- Tmux and graft remain receiver implementations selected outside the runtime.
  The daemon does not import `atm-graft`; the runtime imports neither harness.
- No resend/replay is implemented in AL. A future opt-in replay feature, if
  authorized, starts only after minimal direct cross-host proof and uses the
  same endpoint and types.

## Sprints

### AL.1 — Contract gate and runtime crate skeleton

**Depends on:** AK.11 hook-contract merge/transplant.

- Add workspace library crate `atm-http-runtime` with a minimal public API:
  typed server construction, typed client construction, listener/connector
  configuration, and graceful shutdown handle.
- Add Tokio, Axum/Hyper, Rustls, and the selected maintained client through
  workspace dependencies with minimal feature sets.
- Make the runtime depend only on core interfaces and protocol types.
- Add compile-time/boundary tests proving it cannot import SQLite, tmux, graft,
  daemon-bootstrap, or resend modules.

**Accept when:** the crate compiles; active hook type is
`MessageReceivedHookEmitter`; the boundary checklist is encoded in tests; no
production request flows through the new crate yet.

### AL.2 — Canonical typed HTTP handler

**Depends on:** AL.1.

- Implement `POST /v1/atm/messages` with framework JSON extraction of the
  existing `RequestEnvelope` and framework serialization of
  `ResponseEnvelope`.
- Inject `ApiRouter`, storage-facing core contracts, authenticated ingress
  provenance, observability, and the received-hook boundary as explicit state.
- Ensure all authentication/provenance normalization happens before the one
  core dispatch call; there is no peer decoder or peer router.
- Add error mapping from typed core errors to the existing HTTP result contract
  without ad-hoc framing or second response schemas.

**Accept when:** local and peer fixtures dispatch identical serialized writes;
the route rejects malformed standard HTTP through the framework; no handwritten
HTTP parser/writer is added.

### AL.3 — Post-persistence received-hook semantics

**Depends on:** AL.2 and the AK.11 hook contract.

- Connect the runtime to the one post-persistence path rather than a sender
  hook or listener callback.
- Preserve idempotency: a duplicate message ID is an informational successful
  result and does not emit a second hook.
- Return successful write/receive result plus warning information when the
  injected hook fails; retain diagnostic cause locally.
- Provide a test emitter that proves invocation count and error behavior.

**Accept when:** all three hook proofs in the shared checklist pass against
the runtime handler for single and multiple independently delivered requests.

### AL.4 — Shared standard client and physical adapters

**Depends on:** AL.2.

- Implement one typed send function that serializes the existing envelope,
  sends it to `/v1/atm/messages`, and decodes the existing response envelope.
- Adapt Unix UDS, loopback TCP, and authenticated cross-host TLS by connector
  configuration only. No adapter may own a separate request or response codec.
- Migrate CLI/graft local client use and cross-host caller use to this API.
- Start only framework-managed Tokio listeners. Connection limits, deadlines,
  tracing, and shutdown are configured in the runtime rather than represented
  by ATM-owned threads or coordinators.

**Accept when:** direct local and direct cross-host sends share the exact
client serialization and outcome path; no automatic retry/replay starts.

### AL.5 — Daemon composition, proof, and performance gate

**Depends on:** AL.3 and AL.4.

- Reduce `atm-daemon` integration to building trait implementations,
  selecting listener/connector configuration, starting the runtime, and
  graceful shutdown.
- Prove local CLI, localhost/self target, and M5 cross-host direct sends use
  the new listener and canonical route.
- Run the full test suite, local smoke, M5 smoke, and a comparable benchmark.
  Report measured results; no unverified performance target is asserted.
- Produce AM's removal ledger from actual live references only.

**Accept when:** all QA proof-set items pass, the M5 team has reproduced the
cross-host proof from a clean checkout, and the only legacy references left
are the explicit AM removal ledger.

## Explicitly deferred

- Automatic resend, heartbeat-driven recovery, cursor tracking, batching, and
  `message[]` delivery are not AL features.
- New notification modes and changes to tmux/graft UX are not AL features.
- Storage schema changes and daemon knowledge of SQLite are prohibited.

## Phase completion gate

AL completes only when the new runtime is the proven active path and satisfies
every required row of the shared boundary checklist. It does not complete by
merely compiling alongside the legacy server. AM may remove the old stack only
after this gate passes.
