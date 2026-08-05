# ADR-047 — Direct Trusted-LAN Peer HTTP

| Field | Value |
| --- | --- |
| ID | ADR-047 |
| Status | Accepted |
| Scope | Repository-wide |
| Supersedes | ADR-034, ADR-040, ADR-041 |
| Amends | ADR-035 |

## Decision

ATM peer delivery is one direct plain-HTTP request over one `TcpStream` after
the origin SQLite admission commits. The configured canonical hostname and
port are the sole connection input; the operating system resolves the
hostname. ATM neither saves an address nor enumerates addresses, starts a DNS
thread, reloads SQLite, scans peers, creates an outbound worker, or retries.

The origin emits the exact immutable `WriteRequest`, including its origin ULID
and timestamp, through the shared HTTP frame writer. The configured local
advertised host is attached as `X-ATM-Peer-Source-Host`. It is display
provenance only, not authentication or routing input. The configured peer
listener accepts that write as `WriteIngress::Peer`, performs the existing
provenance validation and canonical admission, then takes the same ordinary
post-write nudge path used for loopback, same-IP, and cross-host frames.

The same existing `POST /v1/atm/messages` peer listener also accepts the
peer-only bounded non-empty body `PeerMessageArray { messages: Vec<WriteRequest> }`.
This is not a second route, listener, receiver service, or nudge path. The
listener attaches its authenticated source-host provenance to every member and
the shared handler validates and prepares every member before calling the
existing atomic message-store batch boundary once. One success response means
the full array committed; invalid, conflicting, or storage-failed arrays
produce no newly accepted subset. Sender-side array emission and whole-array
confirmation are intentionally separate concerns.

After that commit each peer receipt uses the normal local post-write route.
Queue saturation, nudge, hook, or notification failures are observable
post-commit warnings; they never revise a successful peer receive into a
failure and are never a sender receipt.

One matching `ResponseEnvelope::Send` confirms delivery. The origin then
atomically removes only that message's `peerOutbound` marker for the matching
canonical host. Any connect, write, read, frame, response, or confirmation
failure returns `REMOTE_DELIVERY_UNCONFIRMED` after local persistence and
leaves the marker intact. This is a no-retry baseline; later retry policy must
call the same finite-frame function and may not create another sender path.

Peer listeners bind only a finite, enabled, explicit local-address allowlist.
Startup rejects empty, duplicate, wildcard, multicast, or non-local bind
addresses. The listener has a 64-connection cap and uses bounded HTTP framing;
it owns no outbound lifecycle. Network isolation outside explicit binding is a
deployment/firewall responsibility.

## Consequences

- TLS, certificate pinning, custom resolver behavior, and the retired peer
  coordinator are not active delivery-path concerns.
- An unavailable configured peer is a clear, durable-but-undelivered result,
  not a background task or a local success.
- Local, loopback, same-IP, and cross-host write frames converge on one
  receiver and nudge path. Only self-send validation is origin-side special
  handling.
- Peer array ingress is receiver-side-only: it extends the existing write
  body decoder and atomic storage call, not origin routing or receiver
  lifecycle.
- mTLS remains an interop-only preservation fixture until a separately scoped
  future transport decision adopts it; it is not a production fallback.
