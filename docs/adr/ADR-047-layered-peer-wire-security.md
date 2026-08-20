# ADR-047 — Layered Peer-Wire Security

| Field | Value |
| --- | --- |
| ID | ADR-047 |
| Status | Accepted |
| Scope | Repository-wide |
| Relates to | ADR-033, ADR-034, ADR-035, ADR-040; REQ-CORE-TRANSPORT-002B1; REQ-DAEMON-TRANSPORT-002B1; Phase AO |

## Decision

Peer-wire security is selected once for each daemon process by the typed
`PeerWireMode` launch policy. `PeerWireSecurity::Mtls` is the default and
normal mode. `PeerWireSecurity::PlaintextTest` is available only through the
explicit non-durable daemon launch argument
`--peer-wire-security plaintext-test` for bounded diagnostics and benchmarks.
It is untrusted test provenance, never authentication or authorization.

The mode is an outer stream-adapter decision. It does not select an HTTP
resource, request DTO, canonical write handler, persistence operation,
acknowledgement sender, post-write hook, or benchmark-only daemon. Plaintext
mode uses the preserved `DirectPeerTcpConfig::standard`,
`DirectPeerTcpConnector`, direct-peer listener, and ordinary router pipeline.
The mTLS adapter wraps that same pipeline at the stream boundary only.

No environment variable, durable setting, TLS adapter availability check, or
TLS/certificate/allowlist failure may choose or change the selected mode. A
normal restart selects mTLS; mTLS never falls back to plaintext. In particular,
the plaintext pipeline remains direct and unchanged even when mTLS support is
compiled into the same binary, so an enabled TLS feature cannot impose work on
the plaintext hot path.

Only daemon-launch parsing may construct a process mode from user input. An
unknown mode returns `ATM_PEER_WIRE_MODE_INVALID`; durable or environment
selection returns `ATM_PEER_WIRE_MODE_SOURCE_FORBIDDEN`. Work that requires
authenticated peer evidence fails closed with
`ATM_PEER_WIRE_PLAINTEXT_AUTHENTICATION_REQUIRED` when plaintext-test is
selected.

## Consequences

ADR-034's transport-security sequencing and ADR-040's authority-resolution
wording are superseded by this mode-layering decision, while their endpoint,
pinning, and authority rules remain in force. ADR-035 remains the active
canonical ingress decision; this ADR supersedes only its peer-wire transport
wording. ADR-033's one-router contract remains unchanged.

Doctor, retained diagnostics, smoke JSON/XHTML, and benchmark evidence must
record the active peer-wire mode. Plaintext-test evidence cannot satisfy any
mTLS or peer-allowlist acceptance criterion.
