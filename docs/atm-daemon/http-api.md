# ATM daemon HTTP interface

| Field | Value |
| --- | --- |
| Status | Proposed — Phase AI target |
| Version prefix | `/v1/atm` |
| Authoritative ADR | ADR-033 |
| Machine-readable publication | checked-in OpenAPI 3.1 and `atm api spec` |

## Transport and handler rule

The same REST router serves HTTP over local UDS and HTTPS over TCP. A local
client supplies the structured caller address in the canonical write request;
the shared handler validates it under the local caller/roster policy. HTTPS
authentication establishes the peer identity for transport authorization and
never rewrites that caller address. The router maps resources to shared
application handlers only. It does not touch SQLite, choose a host, or emit
nudges.

Every message projection uses structured `from` and `to` addresses. Each has
`agent`, optional `chat_id`, `team`, and optional `host`. Storage keeps the
optional chat IDs in separate nullable source/destination columns; CLI, graft,
Python, nudge, and read rendering show a present value as `agent:chat-id`.
Thus `hendrix:12345@hermes` and `hendrix:98765@hermes` are independent inbox
and reply identities. `chat_id` is not a daemon session or a message-thread
field.

Remote HTTPS requires mTLS plus the configured exact peer identity and pinned
certificate fingerprint. Local UDS uses endpoint ownership/permissions. These
are adapter concerns and do not alter endpoint schemas.

## Resource contract

| Endpoint | Method | Meaning | Shared handler |
| --- | --- | --- | --- |
| `/v1/atm/messages` | `GET` | List/query visible messages; non-mutating | read/query |
| `/v1/atm/messages` | `POST` | Create/send immutable message | canonical write |
| `/v1/atm/message/{message-id}` | `GET` | Inspect one message; non-mutating | read/query |
| `/v1/atm/message/{message-id}` | `DELETE` | Clear one message where authorized | clear |
| `/v1/atm/message/{message-id}/read` | `POST` | Owner-only read-state mutation | read mutation |
| `/v1/atm/message/{message-id}/ack` | `POST` | Acknowledge via canonical write | canonical write |
| `/v1/atm/doctor` | `GET` | Return safe daemon/transport health | doctor |
| `/v1/atm/teams` | `GET` | List teams visible to the caller | team query |
| `/v1/atm/teams` | `POST` | Create a team where authorized | team administration |
| `/v1/atm/team/{team-name}` | `GET` | Fetch one team and its safe metadata | team query |
| `/v1/atm/team/{team-name}` | `PATCH` | Authorized team metadata update | team administration |
| `/v1/atm/team/{team-name}` | `DELETE` | Authorized team removal | team administration |
| `/v1/atm/team/{team-name}/members` | `GET` / `POST` | List/add members | roster administration |
| `/v1/atm/team/{team-name}/member/{agent-name}` | `PATCH` / `DELETE` | Update/remove a member | roster administration |

`GET /v1/atm/messages` accepts independent `agent` and `chat_id` query
filters. `agent=hendrix` searches that base agent across every chat identity;
`agent=hendrix&chat_id=12345` narrows to `hendrix:12345`. Filters apply to the
selected participant direction when a direction is requested, otherwise to
either message participant. They do not alter the authenticated caller.

`POST /v1/atm/message/{message-id}/ack` constructs the same `WriteRequest` used by
`POST /messages`, with only `acknowledges_message_id` populated. The receiver's
canonical write handler owns both persistence and acknowledgement mutation.
It carries the message's full chat-qualified source address as the reply
destination without a separate acknowledgement route.

## Response rules

- JSON is the only application representation in v1.
- Successful reads return the canonical domain projection; collection results
  carry an opaque cursor and a server-selected bounded page size.
- Successful creation returns `201 Created`, the immutable message identity,
  and a `Location` pointing to `/message/{message-id}`.
- Idempotent creation of an existing message ULID returns the existing resource
  projection without a second persistence or nudge event.
- Every failure uses ADR-032's JSON `{ "code", "message" }` error shape.
- Mutation preconditions and authorization failures use ordinary HTTP status
  codes but retain the same ATM error code in the body.

## Publication and compatibility

`docs/atm-daemon/openapi.yaml` is the source artifact. CI validates the
OpenAPI document against route schemas and tests every documented route. The
embedded document is published by `atm api spec --format json|yaml`; no daemon
network endpoint is needed merely to retrieve documentation.

The v1 resource paths are durable. Additive fields are allowed. Removing or
changing a field, status meaning, authorization rule, or handler mapping needs
a new API version and ADR review.

## Deferred scope

Browser session authentication/authorization UI, CORS policy, static assets,
and a web frontend are a later phase. They consume this API; they do not add a
second application handler or a browser-specific persistence path.
