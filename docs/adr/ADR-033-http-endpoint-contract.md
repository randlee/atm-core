# ADR-033 — HTTP Endpoint Contract

| Field | Value |
| --- | --- |
| ID | ADR-033 |
| Status | Accepted |
| Scope | Repository-wide |
| Relates to | ADR-003, ADR-032, ADR-035, Phase AI |

## Decision

ATM uses one daemon HTTP router. Unix local clients use HTTP over UDS and may
use HTTP over loopback TCP; Windows local clients use HTTP over loopback TCP
only. Normal remote peers use HTTPS over TCP. The explicit daemon-only
`--peer-wire-security plaintext-test` profile may carry that same remote HTTP
resource over TCP solely for smoke diagnosis; it is not peer authentication or
a second API. Every transport calls the same router and the same application
handlers.

The initial stable application surface is resource-oriented REST under
`/v1/atm`:

| Resource | Initial operations |
| --- | --- |
| `/messages` | `GET` list/query, `POST` canonical send/ack write |
| `/messages/inspect` | `POST` non-mutating inspection/query |
| `/messages` | `DELETE` clear selected messages where authorized |
| `/messages/read` | `POST` owner-only read-state mutation |
| `/doctor` | `GET` doctor report |
| `/peers/{peer}/sync` | `POST` one explicit bounded replay of immutable stored writes for a registered peer |
| `/runtime/reload` | `POST` reload the authenticated runtime view after local trust/configuration changes |
| `/compatibility` | `POST` compatibility preflight |
| `/heartbeat` | `POST` runtime heartbeat |

The checked-in OpenAPI 3.1 document defines typed route-specific JSON bodies,
status codes, pagination, and conditional mutation semantics before
implementation. The HTTP wire body is never a generic
`RequestEnvelope`/`ResponseEnvelope`; failures use ADR-032's `{code,message}`
body with the route's HTTP status. `POST /messages` builds the same internal
canonical write for send and acknowledgement; an acknowledgement only has
`acknowledges_message_id: Option<MessageId>` populated. It is not a separate
resource, envelope, transport, or persistence path.
`/message/{message-id}` is a `Location` identifier only; v1 does not register a
separate message-by-id route.

The HTTP API has an independent strict SemVer identity. Its major equals the
`/v{major}` path segment. Same-major minor additions are compatible and patch
releases are corrective only: servers accept omitted additive request fields
using documented defaults and ignore unknown additive fields; clients tolerate
additive response fields and error details. A client may require an explicitly
advertised capability for a new operation, but it must not reject an otherwise
supported existing operation solely because the peer reports a different minor
or patch. Product release versions are diagnostic metadata, not HTTP
compatibility input.

The canonical write request contains ADR-037's structured caller and
destination `AgentAddress`: `agent`, optional `chat_id`, `team`, and optional
`host`. The shared handler validates the local caller under the normal
caller/roster policy; HTTPS authentication identifies the peer and never
rewrites caller or destination fields. The API preserves these fields exactly
in a message projection. It must not reduce a chat-qualified address to its
base agent or invent a session header as a parallel identity contract.

Nudge is a post-write internal event. It is not committed as a third public
verb or endpoint until an adapter inventory proves that a remotely invocable
nudge endpoint is necessary. Any future endpoint must call the existing
post-write event boundary rather than recreate write or mailbox logic.

The contract is web-ready: versioned JSON request/response schemas and OpenAPI
3.1 are durable API artifacts, not CLI documentation. A future web interface
is another authenticated HTTP client of these routes; it is not Phase AI scope.
The route inventory is maintained in
[`docs/atm-daemon/http-api.md`](../atm-daemon/http-api.md).

Compatibility, health, and administrative operations are separately inventoried
before migration. They may become HTTP routes only when they need daemon
dispatch; they must not preserve legacy framing merely to avoid the decision.

## Consequences

CLI aliases and graft are presentation clients of these resource endpoints.

The HTTP adapter owns HTTP status/header translation and ingress authentication
only. Local loopback TCP authenticates with a daemon-created, owner-readable
runtime endpoint record and a 32-byte base64url capability in
`X-ATM-Local-Capability`; it binds only a loopback address. Unix UDS uses
owner-only endpoint permissions. Normal HTTPS uses mTLS plus the exact
allowlist. The router receives `AuthenticatedIngress::Local` only after local
capability or UDS ownership authentication, and `AuthenticatedIngress::Peer`
only after mTLS verification. The explicit plaintext-test profile may supply
separately typed untrusted smoke provenance, which cannot authorize a
recipient or claim peer authentication. Socket family and address never
classify an ingress. The adapter cannot perform recipient routing, storage
mutation, acknowledgement mutation, or nudging.

`local-http.json` includes the serving singleton instance ID. A loopback TCP
client verifies that ID against the owner record before connecting; it rejects
missing, stale, revoked, or mismatched metadata. Orderly shutdown records
revocation and syncs it before removing the endpoint publication.

Windows CI proves local loopback-TCP HTTP; Unix CI proves both UDS and
loopback-TCP HTTP. Windows has no alternate local transport or address-derived
fallback.

Local HTTP framing is stream-stateful: an adapter may receive part of a frame
or multiple frames in one system read. It uses one bounded buffer, detects the
header delimiter within received chunks, and retains bytes after a completed
frame for the next request. A delimiter implementation may be optimized, but
it must not regress to one system read per byte or discard a coalesced frame.
The portable scalar delimiter search is the required behavior on every target.
Any library-provided runtime SIMD dispatch is optional and must produce the
same frames and errors as scalar parsing; no ATM build or supported CPU depends
on SIMD support.
