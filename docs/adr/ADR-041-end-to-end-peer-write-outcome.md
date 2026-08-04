# ADR-041 — End-To-End Peer Write Outcome

| Field | Value |
| --- | --- |
| ID | ADR-041 |
| Status | Superseded by ADR-047 |
| Scope | Repository-wide |
| Relates to | ADR-032, ADR-034, ADR-035, Phase AI.26–AI.31 |

## Decision

Every daemon request owns one absolute `RequestDeadline` for local admission.
The SQLite admission transaction is the sole synchronous post-validation
operation before the local response. For an acknowledgement it inserts the
immutable reply and conditionally transitions its source in that same
transaction. A daemon-owned, reloadable in-memory admission view supplies
only already-loaded routing data; the response path never reads a caller
workspace, post-send hook configuration, peer policy, or outbound page.
Background delivery and local nudge work use identifier-only non-durable
signals with separate worker budgets. DNS, connection, TLS, hook/graft I/O,
and remote receipt cannot delay or be cancelled by the completed local
response. The immutable origin record is the worker's only durable input.

For a remote write, local persistence is not delivery success. The only
successful remote result is a verified HTTP response from the peer after its
canonical write handler accepts the request. Any deadline, disconnect, or
response-write failure after outbound dispatch returns one typed
`REMOTE_DELIVERY_UNCONFIRMED` error: the sender record remains immutable and
the caller must treat receiver acceptance as unknown. Repeating the same
immutable ULID is safe through ordinary idempotent write handling.

`ATM_DAEMON_UNAVAILABLE` is reserved for an unavailable local daemon. A local
response-read timeout must never map to that code when the daemon accepted the
request. In particular, peer work must not hold the local response open after
local admission. Daemon connection handler failures, terminal route errors,
and response-write failures are structured retained events.

Observability names outcomes precisely:

- `write_persisted` means only the origin write committed;
- `peer_delivery_confirmed` requires the peer HTTP acceptance response; and
- `peer_delivery_unconfirmed` records deadline/disconnect/failure with the
  message ULID and typed error code.
- `peer_delivery_expired` records that an unconfirmed immutable record aged
  out of the explicitly enabled reconciliation window. It is a terminal
  observability outcome, not a delivery receipt or a change to the earlier
  local admission response.

No event may label local persistence as `sent` or remote delivery.

## Consequences

The daemon keeps one tracked canonical write path and no outbox, replay queue,
receipt, or sender-side acknowledgement state. The client receives an honest
result even when a remote side effect is inherently uncertain.
