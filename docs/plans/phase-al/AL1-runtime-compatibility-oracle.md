# AL.1 Runtime Compatibility Oracle

Source baseline: `develop` at `cd03ef337b36ca785dd15270f16cdc2e96ec89cf`.

This record is deliberately an inventory, not a new wire contract. AL.1 adds
no request DTO, response DTO, JSON field, envelope, route, or serializer.
AL.2 must consume only the listed existing entry points.

## Route, schema, and serializer inventory

The checked-in OpenAPI source is
[`docs/atm-http-runtime/openapi.yaml`](../../atm-http-runtime/openapi.yaml). Every route
below uses its existing route-specific JSON request and result type; the
single existing serializer/decoder entry points are
`atm_core::api::{encode_request_body, decode_route_request, encode_response}`.
Those functions keep `RequestEnvelope` and `ResponseEnvelope` in-process only.

| HTTP route | Existing request / successful result | OpenAPI operation |
|---|---|---|
| `GET /v1/atm/messages` | `ListQuery` / `MessageList` | `listMessages` |
| `POST /v1/atm/messages` | `WriteRequest` / `SendOutcome` or `AcknowledgedOutcome` | `writeMessage` |
| `DELETE /v1/atm/messages` | `ClearQuery` / 204 + `X-ATM-Clear-Outcome` | `clearMessages` |
| `POST /v1/atm/messages/inspect` | `PeekQuery` / `MessageList` | `inspectMessages` |
| `POST /v1/atm/messages/read` | `ReadQuery` / `MessageList` | `readMessages` |
| `GET /v1/atm/doctor` | `DoctorQuery` / doctor projection | `doctor` |
| `POST /v1/atm/peers/{peer}/sync` | `PeerSyncRequest` / `PeerSyncOutcome` | `syncPeer` |
| `POST /v1/atm/compatibility` | `CompatibilityPreflight` / `CompatibilityVerdict` | `compatibilityPreflight` |
| `POST /v1/atm/heartbeat` | `TeamMemberHeartbeatRequest` / heartbeat result | `teamMemberHeartbeat` |
| `POST /v1/atm/runtime/reload` | `()` / `()` | `reloadRuntimeView` |

`AtmError` is the only failure body for all entries; its status selection is
performed by the same `encode_response` entry point (400 for validation,
503 otherwise). No AL.1 runtime type appears in this table or in OpenAPI.

| Concern | Existing owner / proof | AL.1 decision |
|---|---|---|
| Request body and route | `atm_core::api::ApiRequest`, route descriptors in `crates/atm-core/src/api.rs` | Reuse unchanged; do not expose `RequestEnvelope` as a generic HTTP body. |
| Successful result | `atm_core::api::ApiResponse` and route-specific response serializers | Reuse unchanged. |
| Error schema | `AtmError` serializes the ADR-032 `{code,message,cause?}` contract | AL.2 must map all framework rejections through this existing schema. |
| Warning representation | `send::WriteOutcome::{Sent,Acknowledged}` contains existing `WarningEntry` collections | Representable. A failed received hook appends one existing warning after persistence; no schema change is authorized. |
| New / duplicate / conflict disposition | `send::DeliveryPersistenceResult` plus `DuplicateWriteDisposition::{NotDuplicate,AlreadyDeliveredRemote,SameStorePeerReceipt}` | Existing result distinguishes new, idempotent duplicate, and conflict. No core trait change is authorized. |
| Client seam | sealed `atm_core::api::DaemonApiClient::execute` and its allowlisted implementations | AL.4 converts this one existing trait to `async_trait`; AL.1 adds no client trait. |

## `DaemonApiClient` allowlist frozen for AL.4

The exact existing trait is
`DaemonApiClient::execute(&self, ApiRequest) -> Result<ApiResponse, AtmError>`
at `crates/atm-core/src/api.rs`.  The sealed implementation set at the AL.1
baseline is exhaustive and is the only set AL.4 may migrate in the same
change:

| Implementation | Source | Role |
|---|---|---|
| `LocalIpcClientTransportAdapter` | `crates/atm/src/composition.rs` | CLI local daemon adapter |
| `GraftLocalIpcClientTransport` | `crates/atm-graft/src/transport.rs` | Graft local daemon adapter |
| `FakeClientTransport` | `crates/atm-core/src/transport/testing.rs` | deterministic core test double |
| `LoopbackClientTransport` | `crates/atm-core/src/transport/testing.rs` | in-process core test adapter |

AL.4 must migrate all four and must not add an implementation, an async bridge,
or another client trait. This inventory is intentionally separate from
`ApiRouter` implementations: the latter are server-side dispatcher adapters,
not outbound clients.

## Negative-response fixture capture

The source audit intentionally found an existing adapter divergence. This is
the compatibility decision AL.2 must resolve explicitly rather than inherit
from Axum defaults:

| Fixture | Local UDS/loopback legacy path | Legacy TLS peer path | Required AL.2 decision |
|---|---|---|---|
| malformed JSON/body-route mismatch | `local_ipc_transport::enqueue_request` returns `AtmError`; `local_tcp_transport::enqueue_tcp_request` serializes `ResponseEnvelope::Error` | `https_transport::handle_tls_connection` propagates `decode_request` with `?`, closing without a response | Select one ADR-032 response behavior for all runtime adapters and capture byte fixtures before activating the replacement. |
| oversized declared body | `HttpFrameReader::read_request` rejects above `MAX_HTTP_REQUEST_BODY_BYTES`; local adapters convert a post-read decode failure to `ResponseEnvelope::Error` where it reaches their enqueue path | `read_http_request(stream)?` propagates the error and closes the TLS connection | Framework body limit rejection must be wrapped in the selected ADR-032 mapper, never Axum plain text. |
| invalid `X-ATM-Peer-Source-Host` | not applicable to authenticated local UDS capability path | `https_transport::handle_tls_connection` returns validation error before `decode_request`, so the connection closes | Define and snapshot the same selected error response. |

`write_http_response(ResponseEnvelope::Error(AtmError::validation(...)))`
already proves the canonical structured form: HTTP `400 Bad Request`, direct
serialized `AtmError` JSON (no wrapper), with code/message/cause according to
ADR-032. The current focused regression is
`atm_core::api::tests::http_error_is_direct_error_body_with_non_success_status`.

The concrete malformed request bytes and their captured legacy dispositions
are retained beside this record:

- [`fixtures/malformed-json.http`](fixtures/malformed-json.http) — local
  adapter's direct ADR-032 400 response; the TLS peer adapter closes because
  its legacy decoder error was propagated;
- [`fixtures/oversized-body.http`](fixtures/oversized-body.http) — local
  adapter's direct ADR-032 400 response after the existing 1 MiB frame limit;
  the TLS peer adapter closes before response serialization;
- [`fixtures/invalid-peer-source-host.http`](fixtures/invalid-peer-source-host.http)
  — authenticated peer validation failure and legacy TLS close disposition.

These are baseline artifacts, not a new protocol. AL.2 must turn the selected
uniform behavior into byte-level runtime tests; it may not silently inherit
Axum's plain-text extractor/default rejection bodies.

AL.2 is blocked from accepting framework defaults for these cases. It must add
byte-level fixtures for the selected behavior and verify the same status and
ADR-032 JSON body through the maintained HTTP runtime.

## Archived hook provenance

- archived source: `88bca9d5e232006339f43a4e97eef335531b8a8f`;
- copied sealed signature: `MessageReceivedHookEmitter::emit_post_send(&BuiltInPostSendDispatch) -> Result<PostSendEmissionPath, AtmError>`;
- AL.1 copy scope: receiver-hook boundary, tmux/Graft receiver implementations,
  associated manifests, exports, and documentation only;
- explicitly excluded: peer listener/client/decoder, array wire grammar,
  scheduler/replay/coordinator, and legacy transport deletion.

The runtime facade is validated lifecycle-only in AL.1: it owns no listener,
endpoint publication, HTTP decoder, or client migration. Those are separately
owned by AL.2 and AL.4–AL.8.
