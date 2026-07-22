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
only. Remote peers use HTTPS over TCP. Every transport calls the same router
and the same application handlers.

The initial stable application surface is resource-oriented REST under
`/v1/atm`:

| Resource | Initial operations |
| --- | --- |
| `/messages` | `GET` list/query, `POST` create/send |
| `/message/{message-id}` | `GET` non-mutating inspection, `DELETE` clear where authorized |
| `/message/{message-id}/read` | `POST` owner-only read-state mutation |
| `/message/{message-id}/ack` | `POST` acknowledgement |
| `/doctor` | `GET` doctor report |

The checked-in OpenAPI 3.1 document defines typed route-specific JSON bodies,
status codes, pagination, and conditional mutation semantics before
implementation. The HTTP wire body is never a generic
`RequestEnvelope`/`ResponseEnvelope`; failures use ADR-032's `{code,message}`
body with the route's HTTP status. An acknowledgement endpoint builds the same
internal canonical write whose `acknowledges_message_id: Option<MessageId>` is
populated. It is not a separate envelope, transport, or persistence path.

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
owner-only endpoint permissions. HTTPS uses mTLS plus the exact allowlist. The
router receives `AuthenticatedIngress::Local` only after local capability or
UDS ownership authentication, and `AuthenticatedIngress::Peer` only after
mTLS verification; socket family and address never classify an ingress. The
adapter cannot perform recipient routing, storage mutation, acknowledgement
mutation, or nudging.

Windows CI proves local loopback-TCP HTTP; Unix CI proves both UDS and
loopback-TCP HTTP. Windows has no alternate local transport or address-derived
fallback.
