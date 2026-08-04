# ATM daemon HTTP interface

| Field | Value |
| --- | --- |
| Status | Proposed — Phase AI target |
| HTTP API SemVer | `1.0.0`; major is `/v1/atm` |
| Authoritative ADR | ADR-033 |
| Machine-readable publication | checked-in OpenAPI 3.1 and `atm api spec` |

## Transport and handler rule

The same REST router serves HTTP over Unix UDS, local loopback TCP, and HTTPS
over TCP. Windows uses loopback TCP only; Unix supports both UDS and loopback
TCP. A local client supplies the structured caller address in the canonical write request;
the shared handler validates it under the local caller/roster policy. HTTPS
authentication establishes the peer identity for transport authorization and
never rewrites that caller address. The router maps resources to shared
application handlers only. It does not touch SQLite, choose a host, or emit
nudges.

Every message projection uses structured `from` and `to` addresses. Each has
`agent`, optional `chat_id`, `team`, and optional `host`. Storage keeps the
optional chat IDs in separate nullable source/destination columns; CLI, graft,
nudge, and read rendering show a present value as `agent:chat-id`. AI.18's
Python binding consumes this same projection.
Thus `hendrix:12345@hermes` and `hendrix:98765@hermes` are independent inbox
and reply identities. `chat_id` is not a daemon session or a message-thread
field.

Remote HTTPS requires mTLS plus the configured canonical peer hostname and
pinned certificate fingerprint. A host or literal IP destination is accepted
only through the immutable configured `PeerDirectory`; its canonical hostname
is persisted and no DNS/reverse-DNS discovery occurs at admission. Unix UDS uses endpoint ownership/permissions.
Loopback TCP binds only a loopback address and requires the daemon-created
owner-readable endpoint record plus `X-ATM-Local-Capability` (32-byte base64url
capability). These checks create typed local or peer ingress context before the
router; socket family and address never determine request semantics.

The endpoint record contains the serving singleton instance ID. A loopback
client compares it with the owner record and rejects missing, revoked, stale,
or mismatched metadata before opening a connection. Shutdown syncs a revoked
record before removing it.

All routes have one bounded absolute request deadline and reject a body over
`1_048_576` bytes before decode. HTTPS consumes the remaining request budget;
it cannot start an independent longer connect, handshake, or request deadline.
Both listeners stop accepting new requests during shutdown and drain or cancel
tracked requests within the daemon shutdown deadline.

## Resource contract

The route table below mirrors `atm_core::api::http_route_surface()` (backed by
the `HTTP_ROUTE_SPECS` inventory in `crates/atm-core/src/api.rs`). That
inventory is the routing source of truth; this document and
`docs/atm-daemon/openapi.yaml` are checked against it by the OpenAPI surface
tests.

| Endpoint | Method | Meaning | Shared handler |
| --- | --- | --- | --- |
| `/v1/atm/messages` | `GET` | List/query visible messages; non-mutating | read/query |
| `/v1/atm/messages` | `POST` | Create/send immutable message | canonical write |
| `/v1/atm/messages/inspect` | `POST` | Inspect/query messages without mutation | read/query |
| `/v1/atm/messages` | `DELETE` | Clear selected messages where authorized | clear |
| `/v1/atm/messages/read` | `POST` | Owner-only read-state mutation | read mutation |
| `/v1/atm/doctor` | `GET` | Return safe daemon/transport health | doctor |
| `/v1/atm/runtime/reload` | `POST` | Reload the authenticated runtime view after local trust/configuration changes | runtime reload |
| `/v1/atm/compatibility` | `POST` | Verify client/daemon release compatibility | compatibility |
| `/v1/atm/heartbeat` | `POST` | Publish team-member runtime heartbeat | runtime health |

`GET /v1/atm/messages` accepts independent `agent` and `chat_id` query
filters. `agent=hendrix` searches that base agent across every chat identity;
`agent=hendrix&chat_id=12345` narrows to `hendrix:12345`. Filters apply to the
selected participant direction when a direction is requested, otherwise to
either message participant. They do not alter the authenticated caller.

An acknowledgement uses the same `WriteRequest` sent to `POST /messages`, with
only `acknowledges_message_id` populated. The receiver's canonical write
handler owns both persistence and acknowledgement mutation. The response's
`Location` header identifies the created message; `/message/{message-id}` is
not a separately registered route.

## Response rules

- JSON is the only application representation in v1.
- Successful reads return the canonical domain projection; collection results
  carry an opaque cursor and a server-selected bounded page size.
- Successful creation returns `201 Created`, the immutable message identity,
  and a `Location` identifier. v1 does not register a message-by-id route.
- Idempotent creation of an existing message ULID returns the existing resource
  projection without a second persistence or nudge event. Reusing a ULID with
  different immutable content returns the typed conflict error; it preserves
  the original record and performs no side effect.
- Every failure uses ADR-032's JSON `{ "code", "message" }` error shape.
- A host-qualified origin write is successful once the canonicalized immutable
  message is locally persisted. AK.3 performs no peer delivery; AK.4 defines
  the direct peer-HTTP acceptance and unconfirmed-delivery response contract.
- Mutation preconditions and authorization failures use ordinary HTTP status
  codes but retain the same ATM error code in the body.

## Publication and compatibility

`docs/atm-daemon/openapi.yaml` is the source artifact. CI validates the
OpenAPI document against route schemas and tests every documented route. The
embedded document is published by `atm api spec --format json|yaml`; no daemon
network endpoint is needed merely to retrieve documentation.

The v1 resource paths are durable. Same-major additive fields, error details,
and endpoints require a minor version: servers default omitted additive request
fields and ignore unknown additive fields; clients tolerate additive response
fields. Patch versions are corrective only. A new operation may require an
explicitly advertised capability, but a minor or patch mismatch must not reject
an existing operation. Removing or changing a field, status meaning,
authorization rule, or handler mapping needs a new API major and ADR review.
Product release versions are diagnostic only and are not HTTP admission input.

## Peer authority

Cross-host authority is configured as a canonical hostname, HTTPS port, and
certificate pin. Explicit host and literal-IP aliases select that authority
from the immutable `PeerDirectory`; aliases and canonical hosts are
configuration, not DNS results. The configured canonical hostname and port
remain the TLS authority and the canonical host is the only value persisted in
`peerOutbound.host`. After an `atm peer trust` or `atm peer alias` mutation,
the CLI invokes the authenticated local `POST /v1/atm/runtime/reload` control
operation to atomically install the updated snapshot; it does not start a
second daemon.

`atm doctor --json` projects each configured authority as
`trusted_peers[] = { host, https_port, enabled }`. Alias display is
configuration visibility only: it deliberately excludes any dynamically
resolved address, private-key reference, and certificate material.

## Deferred scope

Browser session authentication/authorization UI, CORS policy, static assets,
and a web frontend are a later phase. They consume this API; they do not add a
second application handler or a browser-specific persistence path.
