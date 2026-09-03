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
acknowledgement sender, or post-write hook. The former feature-gated benchmark
harness is superseded by AO.4: public benchmark targets launch the
same shipped Tokio/Axum `atm-daemon` with only the ordinary
`--peer-wire-security` argument, and the received hook remains active.
Cargo features, a benchmark-only daemon, and a test-only HTTP resource are
forbidden. Plaintext mode uses the preserved `DirectPeerTcpConfig::standard`,
`DirectPeerTcpConnector`, direct-peer listener, and ordinary router pipeline.
The mTLS adapter wraps that same pipeline at the stream boundary only.

AO.2 implements that outer adapter in `peer-tls`: the crate consumes the
storage-neutral `PeerConfigStore` and yields authenticated TCP byte streams.
It owns certificate validity, durable-hostname, and exact-pin checks, but it
does not compose a daemon, HTTP route, or message pipeline. Runtime wiring is
explicitly deferred to AO.3, so compiling the adapter cannot alter either
the normal daemon default or the preserved plaintext benchmark path.

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

## AO.3 implementation evidence

AO.3 implements the launch seam in `atm-daemon-bootstrap`. It parses
`--peer-wire-security` once, defaults to `mutual-tls`, rejects duplicate or
unknown values, and rejects `ATM_PEER_WIRE_SECURITY` rather than treating the
environment as an alternate selector. Bootstrap constructs `peer-tls` only in
the mTLS arm and passes its opaque established-stream seam to
`atm-http-runtime`; the runtime has no Rustls, certificate, pin, or
`PeerConfigStore` dependency.

The CLI and graft submit every `WriteRequest` to their selected local daemon.
For a host-qualified request the daemon performs the durable admission and
then uses the selected stream establishment mode for the one outbound HTTP
request. Plaintext therefore retains the existing direct TCP connector and
listener; mTLS changes only the outer byte stream. Inbound plaintext is
explicit `UntrustedSmoke` provenance, while mTLS supplies the exact configured
host only after the client certificate/pin verification completes before HTTP
decode. Startup writes the selected public mode to the retained observability
port and daemon doctor context; neither includes key material, certificate
contents, pins, or raw trust records.

## Legacy literal-IP trusted-peer admission (issue #972)

Once mTLS is selected, `peer-tls` fails closed at startup when any *enabled*
trusted-peer row predates durable-hostname enforcement and still uses a
literal IP authority (`HostName::is_durable_hostname() == false`). The
resulting `ATM_PEER_CONFIG_VALIDATION_FAILED` error names every offending
host and carries the exact `atm peer trust migrate --map <ip>=<hostname>
--yes` / `atm peer trust revoke --host <ip> --yes` remediation, so recovery
never requires a manual SQLite edit. A *disabled* legacy literal-IP row is
historical only: it is reported with a `tracing::warn!` but never blocks
startup.

`ATM_PEER_TRUST_SKIP_LEGACY_LITERAL_IP` (exact value `"1"`) downgrades
enabled legacy literal-IP rows from fail-closed to skip-with-warning: they
are dropped from the trust catalog (never authenticated, in either
direction) and a `tracing::warn!` names each skipped host. This variable is
an explicit, narrowly-scoped testing/benchmarking opt-out for a mixed
old-IP/new-hostname catalog during migration; it does not exist outside that
scope and must never be treated as a general trust bypass. It is a
trust-catalog admission knob only — it cannot select, or fall back to, a
different peer-wire mode, so it does not conflict with this ADR's
environment-selection prohibition for `--peer-wire-security`.
