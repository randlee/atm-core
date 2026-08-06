# Phase AL/AM — Minimal Tokio Runtime Design

Status: binding design for Phase AL and Phase AM

This document specifies the replacement's mechanics without changing ATM's
application protocol. It resolves an important boundary: **Tokio/HTTP library
code replaces socket and frame mechanics; it does not replace, wrap, or
redesign ATM transport structs or serialization.**

## Compatibility freeze

AL.1 must inventory the existing typed HTTP route bodies, successful results,
warning representation, and ADR-032 error body from the checked-in OpenAPI and
current serializer entry points. That inventory is the compatibility oracle
for every AL sprint. In particular:

- `AgentAddress`, `WriteRequest`, acknowledgement fields, message IDs, and
  canonical projections remain the existing `atm-core` types.
- The public `POST /v1/atm/messages` JSON body and success result remain the
  current route-specific types and exact JSON representation. Their concrete
  names are recorded by AL.1; AL does not create a replacement name or wrapper.
- `AtmError { code, message }`, its route status mapping, and any already
  documented success-warning representation remain unchanged.
- `RequestEnvelope` and `ResponseEnvelope` may remain internal application
  values where the existing code uses them. Per ADR-033, neither becomes a
  generic HTTP wire envelope.

No AL PR may add a peer header that carries application data, a peer body, a
`messages[]` grammar, an alternate result DTO, a compatibility serializer, or
a schema migration. If the required hook warning has no existing successful
route representation, AL.3 is **blocked** pending a separately approved API
contract decision; it must not silently add a field.

## Runtime ownership

```text
existing ATM client request type
   │  unchanged serializer
   ▼
atm-core::DaemonApiClient implementation in atm-http-runtime
   │  maintained Tokio HTTP client; connector selection only
   ▼
UDS / loopback TCP / authenticated TLS TCP
   │  standard HTTP server, same router
   ▼
existing route-specific request type
   │  local capability or UDS ownership / mTLS allowlist
   ▼
AuthenticatedIngress + existing ApiRouter::route(WriteRequest)
   │
   ▼
existing MessageWriter storage trait
   │  newly-persisted disposition only
   ▼
MessageReceivedHookEmitter
   │  warning-only on error
   ▼
existing route-specific result or ADR-032 error body
```

`atm-http-runtime` owns only the bolded transport mechanics in that flow:
framework router/extractors, maintained HTTP client/server, connector setup,
TLS setup, request deadline/cancellation wiring, and translation between the
already-existing HTTP route types and the existing core application call. It
does not own message business rules, storage, nudge implementations, replay,
or daemon singleton policy.

`atm-daemon` remains the executable composition root. It acquires existing
singleton ownership before listener publication, constructs backend-neutral
core trait implementations, chooses enabled listener configurations, injects
the received-hook implementation, starts the runtime, and initiates bounded
shutdown. It has no SQL/Rusqlite import and no HTTP parser, protocol worker,
or peer-specific request dispatcher.

## The one write path

Every direct write uses this exact sequence. A physical connector may differ,
but no application step may differ.

1. CLI, graft, and daemon-to-daemon callers build the existing typed request.
   `DaemonApiClient` selects UDS, loopback TCP, or TLS TCP from the destination
   and runtime configuration; it does not select a route-specific data model.
2. One runtime client operation serializes the existing `POST /messages` body,
   sends the existing path and headers, and decodes the existing success or
   ADR-032 error representation. Connection setup is the only connector
   branch.
3. One framework router selects `POST /v1/atm/messages`. Framework body-size,
   HTTP syntax, and JSON extraction limits run before ATM application code.
4. The transport adapter authenticates the connector and supplies only
   `AuthenticatedIngress`. It cannot derive ingress class from socket address,
   mutate caller/destination data, route a recipient, write storage, or nudge.
5. The typed handler makes exactly one canonical `ApiRouter` write dispatch,
   which reaches the existing `MessageWriter` storage trait. No listener,
   client, peer decoder, or graft code may call storage directly.
6. The write disposition distinguishes new persistence, exact idempotent
   duplicate, and conflict. An exact duplicate is informational and emits no
   second received hook. A conflicting immutable payload follows the existing
   typed conflict result.
7. Only after a **newly persisted** inbound write, the handler invokes the
   injected `MessageReceivedHookEmitter`. It has no sender-side call site.
   Hook failure is recorded and represented as the existing successful-warning
   result; it cannot turn the persisted receive into a failure. The hook must
   use the request's remaining bounded budget and never create a detached
   thread, queue, or retry task.
8. The handler emits exactly the pre-existing serialized success/warning or
   error response. This is the result decoded by the same shared client.

The in-process test adapter enters at step 3 with the same router and handler;
it is not a parallel dispatcher. The same-host `.localhost` or advertised-IP
case follows steps 1–8 over ordinary TLS TCP and is not a special self-send
implementation.

## Minimal internal surface

AL.1 exposes a small construction API, with concrete types selected from
already-existing contracts during the inventory. The following is a shape
constraint, not a request to publish new protocol types:

```rust
// Boundary dependencies are existing core traits and existing route types.
// No storage backend, tmux/graft implementation, or peer scheduler is accepted.
pub struct HttpRuntimeBuilder { /* private */ }

impl HttpRuntimeBuilder {
    pub fn build(self) -> Result<HttpRuntime, AtmError>;
}

pub struct HttpRuntime { /* private server/client/lifecycle fields */ }
```

The runtime's only client implementation is the existing sealed
`DaemonApiClient` application contract. It accepts and returns existing
application types. There is no public `PeerClient`, `PeerWrite`, batch client,
or transport-specific client trait.

The runtime may expose listener/connector configuration internally or through
an existing typed configuration boundary. Such configuration carries only
physical information (UDS path, loopback bind/owner capability, TLS authority,
certificate/trust material through approved trait/view, timeout/cap limits).
It never carries a message body, recipient-routing rule, or delivery state.

## Standard-library mechanics and bounded execution

- Tokio owns runtime scheduling, cancellation, and listener tasks. Axum/Hyper
  own HTTP parsing, framing, response write, and HTTP connection behavior.
  Rustls owns TLS. ATM does not retain a `read`/`write` loop around HTTP bytes.
- The handler derives one absolute deadline from the accepted request and
  passes only its remaining budget into core work and the hook. It does not
  create nested per-leg timers. Timeout/cancellation maps through the existing
  `AtmError` catalog and route status contract.
- Accepted request work is registered with existing runtime drain accounting
  until completion or cancellation. Framework tasks are bounded by the
  documented connection/body/request limits; no untracked spawn, worker
  thread, queue, polling loop, timer, cursor, or coordinator is introduced.
- Shutdown stops accepts, waits or cancels tracked requests at the documented
  deadline, revokes/publishes local endpoint state through the existing
  lifecycle boundary, and then releases singleton ownership. The runtime never
  starts a second daemon or chooses an alternative database/root.

## Physical adapters: only the first hop differs

| Adapter | Adapter-only work | Must be shared after authentication |
|---|---|---|
| Unix local | UDS bind/connect, owner-only permissions | route body/result, router, `ApiRouter`, storage traits, hook, errors |
| Loopback local | loopback bind, endpoint record and local capability | same as Unix local |
| Peer HTTPS | DNS/authority policy, connect, Rustls mTLS and allowlist | same as local |
| Plaintext test profile | explicit untrusted smoke provenance only | same route/handler; cannot authorize recipient or satisfy TLS proof |
| In-process test | framework service invocation without live socket | same router/handler and application contracts |

No table row authorizes a peer listener, peer DTO, alternate persistence path,
or special nudge behavior.

## Required proofs before deletion

AL.8 must capture source-level and runtime evidence that:

1. Existing public transport structs and JSON snapshots are unchanged.
2. Each physical adapter reaches the same router, handler, `ApiRouter`, and
   storage trait call.
3. A newly persisted message calls the received hook once; an exact duplicate
   calls it zero times; a hook failure returns the existing successful warning.
4. The daemon/runtime import neither SQLite/Rusqlite nor tmux/`atm-graft`.
5. A direct cross-host failure returns the existing direct-failure outcome and
   starts no retry/replay task.
6. Unix UDS, loopback TCP, M5 HTTPS, and in-process tests all pass through the
   active AL runtime, with a benchmark comparison against the pre-AL baseline.

AM uses this evidence as its deletion gate. If a legacy dependency remains
necessary for a proof, it stays in the AM ledger until replaced; AM may not
claim deletion by hiding it behind an adapter.
