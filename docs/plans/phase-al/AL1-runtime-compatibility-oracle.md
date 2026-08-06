# AL.1 Runtime Compatibility Oracle

Source baseline: `develop` at `cd03ef337b36ca785dd15270f16cdc2e96ec89cf`.

This record is deliberately an inventory, not a new wire contract. AL.1 adds
no request DTO, response DTO, JSON field, envelope, route, or serializer.
AL.2 must consume only the listed existing entry points.

| Concern | Existing owner / proof | AL.1 decision |
|---|---|---|
| Request body and route | `atm_core::api::ApiRequest`, route descriptors in `crates/atm-core/src/api.rs` | Reuse unchanged; do not expose `RequestEnvelope` as a generic HTTP body. |
| Successful result | `atm_core::api::ApiResponse` and route-specific response serializers | Reuse unchanged. |
| Error schema | `AtmError` serializes the ADR-032 `{code,message,cause?}` contract | AL.2 must map all framework rejections through this existing schema. |
| Warning representation | `send::WriteOutcome::{Sent,Acknowledged}` contains existing `WarningEntry` collections | Representable. A failed received hook appends one existing warning after persistence; no schema change is authorized. |
| New / duplicate / conflict disposition | `send::DeliveryPersistenceResult` plus `DuplicateWriteDisposition::{NotDuplicate,AlreadyDeliveredRemote,SameStorePeerReceipt}` | Existing result distinguishes new, idempotent duplicate, and conflict. No core trait change is authorized. |
| Client seam | sealed `atm_core::api::DaemonApiClient::execute` and its allowlisted implementations | AL.4 converts this one existing trait to `async_trait`; AL.1 adds no client trait. |

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
