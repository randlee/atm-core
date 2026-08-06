# Phase AL/AM Runtime Boundary Checklist

Status: binding implementation and QA checklist

This checklist prevents the Tokio replacement from becoming another transport
subsystem. A PR fails its phase gate when any **MUST NOT** rule is violated.

## Required shape

| Rule | Required evidence |
|---|---|
| Frozen public transport contract | All client and server paths preserve the exact existing route-specific public request, success-result, warning, and ADR-032 error types plus their Serde/OpenAPI serialization. `RequestEnvelope`/`ResponseEnvelope` remain internal only where they already are; ADR-033 forbids a generic HTTP envelope. No peer DTO, peer request body, alternate result schema, wrapper, array grammar, or schema migration exists. |
| One write ingress | Local CLI, graft, loopback, and authenticated cross-host writes reach the same typed `POST /v1/atm/messages` handler and the same `ApiRouter` dispatch. The connector and authenticated provenance are the only permitted transport differences. |
| One standard HTTP implementation | Runtime server and client use Tokio plus maintained HTTP/TLS libraries. The ATM code owns application routing and policy only, never HTTP framing, header parsing, request parsing, response parsing, socket read loops, or socket write loops. |
| Core-only runtime dependencies | `atm-http-runtime` may depend on `atm-core` contracts and standard HTTP/TLS/runtime libraries. It has no SQLite, tmux, graft, CLI, daemon-bootstrap, peer scheduler, or resend dependency. |
| Thin daemon composition | `atm-daemon` constructs implementations of sealed core traits, injects them into the runtime, starts listeners, and shuts them down. It has no concrete SQLite dependency or transport application logic. |
| Receiver-only notification | `MessageReceivedHookEmitter` is the AK.11 core trait. It is invoked once only after a newly persisted inbound message; duplicate/idempotent persistence does not emit again. Its error is retained as a warning and never changes a successful receive into a failure. |
| Harness isolation | Tmux and graft receivers are implementations injected at composition. The runtime and daemon have no direct dependency on tmux or the `atm-graft` client crate. |

## Prohibited shape

- `HttpFrameReader`, handwritten HTTP request/response writers, manual HTTP
  parsers, or manual socket framing.
- A peer-only route, decoder, body (`PeerMessageArray`), header protocol, or
  persistence path.
- `PeerResendScheduler`, peer drain/recovery coordinator, replay queue,
  background resend worker, or per-peer state machine. No automatic replay is
  active in AL or AM.
- A sender-side nudge/notification call. Notification is receive-side only.
- Any direct `rusqlite`/SQLite reference from the daemon or HTTP runtime.
- Compatibility shims that retain a second transport server or client after
  the replacement is live.

## QA proof set

1. Byte-level/typed regression proof that local and cross-host use the same
   existing `POST /v1/atm/messages` route body and success/error serialization;
   no public type or JSON schema changed.
2. Handler proof that one newly stored HTTP message emits exactly one received
   hook; a duplicate message ID emits none.
3. Handler proof that a hook failure returns a successful receive/write result
   with a warning.
4. Static boundary proof that prohibited symbols and direct dependency edges
   are absent.
5. Local and M5 cross-host smoke evidence through the new runtime, followed by
   benchmark comparison against the recorded pre-migration baseline.
