# ADR-041 — End-To-End Peer Write Outcome

| Field | Value |
| --- | --- |
| ID | ADR-041 |
| Status | Proposed |
| Scope | Repository-wide |
| Relates to | ADR-032, ADR-034, ADR-035, Phase AI.23–AI.24 |

## Decision

Every daemon request owns one absolute `RequestDeadline`. The local HTTP
adapter, router, dispatcher, post-write router, and HTTPS adapter consume the
same remaining budget; no layer creates a longer independent peer deadline.
Tracked work is cancelled when that budget expires or the local connection is
closed, except that a remote peer may already have accepted bytes before a
cancellation race completes.

For a remote write, local persistence is not delivery success. The only
successful remote result is a verified HTTP response from the peer after its
canonical write handler accepts the request. Any deadline, disconnect, or
response-write failure after outbound dispatch returns one typed
`REMOTE_DELIVERY_UNCONFIRMED` error: the sender record remains immutable and
the caller must treat receiver acceptance as unknown. Repeating the same
immutable ULID is safe through ordinary idempotent write handling.

`ATM_DAEMON_UNAVAILABLE` is reserved for an unavailable local daemon. A local
response-read timeout must never map to that code when the daemon accepted the
request. Daemon connection handler failures, terminal route errors, and
response-write failures are structured retained events.

Observability names outcomes precisely:

- `write_persisted` means only the origin write committed;
- `peer_delivery_confirmed` requires the peer HTTP acceptance response; and
- `peer_delivery_unconfirmed` records deadline/disconnect/failure with the
  message ULID and typed error code.

No event may label local persistence as `sent` or remote delivery.

## Consequences

The daemon keeps one tracked canonical write path and no outbox, replay queue,
receipt, or sender-side acknowledgement state. The client receives an honest
result even when a remote side effect is inherently uncertain.
