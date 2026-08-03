---
title: AK.4 Direct peer HTTP without retry
status: proposed
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.3
parallel_safe: false
---

# AK.4 — direct peer HTTP without retry

## Closure

Prove the smallest production peer-delivery path before resend caching: use
AK.3's persisted full hostname, send ordinary HTTP once, and let the existing
receiver persist and nudge. Failure returns an error and remains undelivered.

## Fixed contract

```rust
fn send_peer_http_frames(
    config: &PeerHttpRuntimeConfig,
    endpoint: &PeerEndpoint,
    writes: &[WriteRequest],
    deadline: RequestDeadline,
) -> Result<Vec<ResponseEnvelope>, AtmError>;

struct PeerDeliveryConfirmation {
    message_id: AtmMessageId,
    canonical_host: HostName,
}

struct PeerHttpRuntimeConfig {
    source_host: HostName,
}

trait MessageStore {
    fn confirm_peer_delivery(
        &self,
        confirmation: PeerDeliveryConfirmation,
    ) -> Result<bool, AtmError>;
}
```

The function is the only peer sender. `PeerHttpRuntimeConfig::source_host` is
the configured local interface advertisement, captured at daemon
configuration load/reload; it is never obtained from CLI input or a per-send
configuration query. Immediate delivery calls the function with a
one-element slice; AK.5 passes an oldest-first durable batch to this exact
function. It uses the shared `RequestEnvelope::Write` HTTP framing helpers
(`write_http_request_with_headers` and `read_http_response_with_frame_reader`)
and the normal configured peer receiver. The source-host header comes from
configured daemon interface data, never a CLI argument. On the trusted-LAN
MVP receiver it is display provenance, not authentication: the receiver
normalizes it into the existing peer provenance fields, then calls
`validate_write_provenance(WriteIngress::Peer, ...)`. Direct send must not use
`UntrustedSmoke` or an alternate nudge path.

The connect is exactly one bounded `TcpStream::connect` call using
`PeerEndpoint::canonical_host` and `endpoint.port`; the operating system
performs ordinary hostname resolution. ATM does not enumerate candidate
addresses, perform a literal-IP fallback, create a DNS thread, or save a
resolved address. There is no outbound queue, timer, retry state, worker,
channel, peer scan, `HttpsMessageTransport` trait/mock, or alternate receiver
route.

The current `HttpsListenerSet` contains two separable responsibilities. AK.4
keeps its existing bounded inbound HTTP execution and renames the plaintext
part `PeerHttpListenerSet`; AK.6 later deletes only its TLS security half.
`PeerHttpListenerSet` owns one existing accept thread per enabled interface and
uses the existing `ActiveConnectionRegistry` to bound existing request threads
at `MAX_PEER_HTTP_CONNECTIONS`. AK.4 creates no outbound thread and no new
listener/request execution model. Every accepted frame—loopback, same-IP, and
cross-host—runs `route_peer_http_request`, the canonical router, SQLite
admission, and the ordinary post-write nudge exactly once.

## Type and boundary inventory

| Item | AK.4 role |
| --- | --- |
| `send_peer_http_frames` | New sole peer-sender function. It accepts singleton and batch slices; it is not a trait object, service, worker, or pool. |
| `PeerHttpRuntimeConfig` | New immutable local source-host snapshot built at daemon configuration load/reload. It supplies only the display-provenance header; it has no peer lookup, socket, retry, or state. |
| `PeerEndpoint` | AK.3 canonical hostname/port input; AK.4 neither resolves aliases nor stores an address. |
| `WriteRequest`, `RequestEnvelope::Write`, `ResponseEnvelope::Send`, `SendResponseEnvelope`, `RequestDeadline` | Existing canonical wire/request types. Acceptance is only the matching `ResponseEnvelope::Send` outcome for that write ULID; every other response, including `ResponseEnvelope::Error`, is a delivery failure. |
| `TcpStream`, `SocketAddr` | Existing standard-library connection/address values. The OS resolver produces one ephemeral address for one direct call; neither enters ATM storage or state. |
| `HttpFrameReader` and shared HTTP writer helpers | Existing framing boundary. Both production and real-loopback tests use it. |
| `PeerHttpListenerSet`, `PeerHttpListener`, `PeerConnectionAdmission` | Renamed retained inbound HTTP listener lifecycle and its exact accept decision. It is the only production peer receiver; no second handler is created. |
| `ListenerSecurity::PlaintextTest`, `AuthenticatedIngress::UntrustedSmoke` | Existing temporary smoke-only classification deleted from production peer-write handling. AK.4 has no replacement enum: configured trusted-LAN peer writes use the existing `Peer` ingress value. |
| `ActiveConnectionRegistry`, `ActiveConnectionGuard`, `TrackedDispatchHandle`, `MAX_PEER_HTTP_CONNECTIONS` | Existing bounded inbound request execution retained unchanged. These apply only after a peer socket is accepted; they are not outbound delivery state. |
| `route_peer_http_request`, `WriteIngress::Peer`, `validate_write_provenance` | Existing common ingress boundary. AK.4 changes the former plaintext-test provenance classification to trusted-LAN `Peer`; it does not create local/cross-host branches. |
| `PeerDeliveryConfirmation`, `MessageStore::confirm_peer_delivery` | New exact confirmation value and one sealed-storage mutation. `true` removes only matching `peerOutbound` metadata after a successful peer response; `false` is an idempotent already-confirmed no-op, so only undelivered writes appear in a later batch. |
| `AtmError` | Existing delivery-failure type; no new retry/result enum is introduced. |

No `PeerHttpTransport`, `PeerHttpSender` trait, connection manager, pool,
worker, queue, or test transport abstraction is authorized.

## Deliverables

1. Add only `send_peer_http_frames`, using one real bounded socket and the shared HTTP
   framing implementation. It owns one bounded connection/deadline and emits
   one write frame/response per input in order; do not add a transport trait,
   connection pool, background connection lifecycle, or test-only delivery
   substitute. A response count/order mismatch, malformed frame, unexpected
   response variant, error response, connect refusal, write failure, and
   read timeout are all typed delivery failures; none may confirm a write.
   Build one immutable `PeerHttpRuntimeConfig` from configured interface data
   at configuration load/reload; do not query peer configuration per send.
2. Extract/rename the plaintext branch of `HttpsListenerSet` to
   `PeerHttpListenerSet` and make it the configured production trusted-LAN
   listener. Retain its listed bounded accept/request execution unchanged;
   delete the `PlaintextTest`/`UntrustedSmoke` production distinction. A write
   with the configured source-host header is admitted as `WriteIngress::Peer`
   after the existing provenance validation. Do not authenticate or route from
   that header in AK.4.
3. Route a host-qualified admission directly to that function with its
   in-memory immutable write after the canonical SQLite commit; do not reload
   SQLite, read all peers, resolve aliases, or look up a literal address.
4. On any connect/write/read/response failure, leave every admitted origin
   record undelivered and return one delivery error that states persistence
   succeeded. Do not add retry behavior, retry state, an outbox, or nudge at
   the origin. A receiver that accepts a frame uses its ordinary post-write
   nudge path.
5. For every matching `ResponseEnvelope::Send` response, call the one atomic
   `MessageStore::confirm_peer_delivery(PeerDeliveryConfirmation { message_id,
   canonical_host })` mutation. It verifies the matching canonical host and
   removes only `peerOutbound`; it never changes the immutable body, ULID,
   timestamp, read/ACK state, mailbox history, or remote record. A repeated
   confirmation is a harmless no-op. A mismatched ULID, wrong response variant,
   or `ResponseEnvelope::Error` is a delivery failure and retains
   `peerOutbound`. This is not an outbox, receipt table, or delivery-state
   enum.
6. Add real-loopback integration tests using the production sender and
   receiver, including a split response frame and multiple frames on one
   connection. Mocks may observe the receiver only after the real HTTP frame
   parser; they may not replace the sender or receiver transport. Also prove
   successful confirmation removes the message from
   `OutboundMessageQuery::page_for_peer`; failed/partial responses leave only
   their corresponding `peerOutbound` metadata.
7. In this same change, create `ADR-045` for the direct trusted-LAN HTTP
   delivery decision. It supersedes the active transport decisions in
   ADR-034, ADR-040, and ADR-041; ADR-035 remains the canonical ingress ADR
   but is amended to say local, loopback, same-IP, and cross-host ingress have
   one receiver/nudge path. Update `docs/adr/INDEX.md`,
   `docs/requirements.md` (`REQ-CORE-TRANSPORT-002`, `-002A`, `-002B`,
   `-002B1`, `-002C`, `-002D`, `-003`, `-004`, and `-005A`),
   `docs/{architecture,boundaries}.md`,
   `docs/atm-daemon/{architecture,boundaries,http-api,requirements}.md`,
   `docs/atm/{architecture,requirements}.md`, and
   `docs/peer-pair-smoke.md`. The documentation must describe the active
   plain-HTTP trusted-LAN MVP and its no-retry baseline without claiming that
   AK.6 has already deleted legacy TLS code. Update the repository
   `crosshost-curl-plain` runner so it starts the configured production
   `PeerHttpListenerSet` without `plaintext-test`/`UntrustedSmoke`; retain a
   separately named interop-only mTLS fixture lane for AK.6.

## Explicit prohibitions

- No coordinator, outbound per-message thread, DNS thread, outbound worker,
  queue, channel, scan, retry, timer, `peer_sync`, or immediate
  durable-request reload. The explicitly inventoried bounded receiver threads
  are retained, not expanded.
- No source-host value from user input, and no local-host/same-IP incoming
  branch. Local, loopback, same-IP, and cross-host frames use the same
  receiver/nudge path.

## Required validation

- Integration: production send and curl submit the same JSON/provenance and
  receive the same response shape through the configured production
  `PeerHttpListenerSet`; neither test may enable `plaintext-test` or use
  `UntrustedSmoke`.
- Integration: the configured production `PeerHttpListenerSet` admits a
  header-bearing plain HTTP write as `WriteIngress::Peer`; loopback, same-IP,
  and cross-host use this exact receiver/nudge path.
- Integration: a one-element immediate call and a multi-frame call use the
  same function and preserve the original ULID/timestamp per write.
- Integration: DNS resolution failure, connect refusal, receiver `4xx/5xx`,
  truncated/split/malformed response frames, mismatched response count, and a
  response for the wrong ULID retain `peerOutbound`, emit no origin nudge, and
  leave the receiver mailbox unchanged unless that receiver accepted its exact
  frame.
- Integration: only matching `ResponseEnvelope::Send` responses retire exactly their matching
  `peerOutbound`; a later oldest-first query cannot select accepted writes,
  while a failed write remains selectable.
- Smoke: M4→M5 and M5→M4 each prove production send, remote read, acknowledged
  reply, full-host rendering, and one receiver nudge. Run the standard
  `just smoke localhost`, `just smoke local-ip`, `just smoke crosshost-send`,
  `just smoke crosshost-ack`, and `just smoke crosshost-curl-plain` lanes in
  both directions using isolated test homes/databases. Curl is the independent
  request-path proof, not a substitute for production sender evidence.
- `just lint` and `just test` pass.

## Dependencies

Before every AK.4 development/fix round, merge AK.3 into AK.4. Start AK.4 as
soon as AK.3 is pushed; do not wait for QA. AK.4 PR completion waits for AK.3
merge. Push AK.4, then start AK.5 with AK.4→AK.5 merge-forward.
`must_follow` is required because AK.5 retries AK.4's one verified function;
it is not parallel-safe because both own the active peer send path.
