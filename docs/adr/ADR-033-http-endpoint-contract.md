# ADR-033 — HTTP Endpoint Contract

| Field | Value |
| --- | --- |
| ID | ADR-033 |
| Status | Proposed |
| Scope | Repository-wide |
| Relates to | ADR-003, ADR-032, ADR-035, Phase AI |

## Decision

ATM uses one daemon HTTP router. Local clients use HTTP over a Unix-domain
socket (including Windows AF_UNIX); remote peers use HTTPS over TCP. Both
transports call the same router and the same application handlers.

The initial stable application surface is resource-oriented REST under
`/v1/atm`:

| Resource | Initial operations |
| --- | --- |
| `/messages` | `GET` list/query, `POST` create/send |
| `/message/{message-id}` | `GET` non-mutating inspection, `DELETE` clear where authorized |
| `/message/{message-id}/read` | `POST` owner-only read-state mutation |
| `/message/{message-id}/ack` | `POST` acknowledgement |
| `/doctor` | `GET` doctor report |
| `/teams` | `GET` team list and collection administration |
| `/team/{team-name}` | `GET` team detail and authorized team administration |

The exact request/response schemas, status codes, pagination, and conditional
mutation semantics are defined in the checked-in OpenAPI 3.1 document before
implementation. An acknowledgement endpoint builds the same internal canonical
write whose `acknowledges_message_id: Option<MessageId>` is populated. It is
not a separate envelope, transport, or persistence path.

Source and destination use ADR-037's structured `AgentAddress`: `agent`,
optional `chat_id`, `team`, and optional `host`. The API must preserve these
fields exactly in a message projection. It must not reduce a chat-qualified
address to its base agent or invent a session header as a parallel identity
contract.

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

CLI aliases and graft are presentation clients of these resource endpoints. The
HTTP adapter owns HTTP status/header translation only. It cannot perform
recipient routing, storage mutation, acknowledgement mutation, or nudging.

Named pipes are not supported. Windows uses AF_UNIX under the same local UDS
contract and must prove it in Windows CI. There is no named-pipe fallback.
