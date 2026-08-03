---
title: AK.6 Remove legacy peer transport machinery
status: proposed
target: integrate/phase-ak
recommended_agent: Cipher-311d
recommended_model: deep-reasoning
must_follow: AK.5
parallel_safe: false
---

# AK.6 — remove legacy peer transport machinery

## Closure

After AK.4/AK.5 prove the replacement path, delete legacy scans, DNS threads,
and active custom TLS. Preserve only provisioning/configuration and curl
receiver interoperability in `crates/atm-peer-tls-interop`; it is not a native
ATM TLS sender and no active daemon, CLI, graft, or send-path crate may depend
on it.

AK.6 deliberately keeps interop preservation and legacy deletion in one
sprint. The curl-mTLS fixture must be captured and moved before the last active
TLS configuration/data paths are deleted; splitting them would require either
a temporary active dependency on the preservation crate or a PR that deletes
the only reproducible fixture before its replacement is qualified. The work is
sequenced inside this one sprint—capture proof, create/move fixture, prove it,
then delete active transport and finalize docs—but has one atomic PR boundary.

The current deletion boundary is explicit: remove
`peer_resolution.rs`, `runtime_health/peer_authority.rs`,
`HttpsMessageTransport`, `SharedHttpsTransport`, `HttpsTransport`, `TlsIdentity`,
`PinnedClientVerifier`, TLS client connection/open/deliver helpers, and the
daemon composition outbound transport slot. Remove all literal-IP fallback and
every DNS/thread helper. Retain the AK.4 `PeerHttpListenerSet` plain receiver;
it is not TLS machinery. Retain only the data/provisioning APIs needed to construct
the separate interop crate and a curl mTLS receiver fixture there. That crate
is an isolated verification/provisioning utility, not an active transport
dependency.

## Fixed contract

```rust
struct TlsInteropConfig {
    local_certificate: LocalCertificate,
    trusted_peers: Vec<TrustedPeer>,
}

impl TlsInteropConfig {
    fn from_provisioning(
        local_certificate: LocalCertificate,
        trusted_peers: Vec<TrustedPeer>,
    ) -> Result<Self, AtmError>;
}

struct CurlMtlsReceiverFixture {
    bind_addr: SocketAddr,
    config: TlsInteropConfig,
}

enum CurlMtlsFixtureOutcome {
    Accepted,
    RejectedClientCertificate,
}

impl CurlMtlsReceiverFixture {
    fn serve_one(
        &self,
        deadline: RequestDeadline,
    ) -> Result<CurlMtlsFixtureOutcome, AtmError>;
}
```

`CurlMtlsReceiverFixture::serve_one` is a test/interop-only bounded one-shot
operation. Its `bind_addr` is supplied by the fixture harness, never daemon
composition; it has no production caller or lifecycle registration. Neither
type has a send, route, resolver, or background-work method.

## Type and boundary inventory

| Item | AK.6 role |
| --- | --- |
| `TlsInteropConfig` | New interop-only value object containing the existing `LocalCertificate` and `TrustedPeer` provisioning data needed by curl verification. It has no routing or send method. |
| `CurlMtlsReceiverFixture`, `CurlMtlsFixtureOutcome` | New interop-only, synchronous one-shot receiver fixture and its accepted/rejected result for curl proof. It has no production caller, background thread, worker, or daemon lifecycle ownership. |
| `LocalCertificate`, `CertificateFingerprint`, `TrustedPeer` | Existing durable provisioning/configuration values retained only for the interop fixture and configuration display. They do not select a production route. |
| `PeerHttpListenerSet`, `PeerHttpListener`, `PeerConnectionAdmission`, `route_peer_http_request`, `ActiveConnectionRegistry` | AK.4's retained production plain receiver. AK.6 must neither delete it nor add a replacement receiver/thread model. |
| `PeerHttpRuntimeConfig` | AK.4's retained immutable source-host snapshot. AK.6 must retain it without adding a TLS, resolver, or delivery capability. |
| `PeerWireSecurity`, `HttpsMessageTransport`, `SharedHttpsTransport`, `HttpsTransport`, `TlsIdentity`, `PinnedClientVerifier`, TLS `ListenerSecurity::MutualTls`, TLS handshake helpers | Existing TLS transport types to delete from active code. `HttpsListenerSet` has already been renamed to `PeerHttpListenerSet` in AK.4; its plain receiver survives. |
| `peer_resolution` / `peer_authority` helpers | Existing legacy resolver/authority surfaces to delete; AK.3 `PeerDirectory` is the sole alias mechanism. |

No other interop trait, service, executor, listener, sender, thread, task,
channel, or production dependency is authorized without a plan amendment.

## Deliverables

1. Create `crates/atm-peer-tls-interop` with exactly the fixed contract above.
   Capture the existing fixture proof, then move verified TLS provisioning,
   certificate/fingerprint configuration/data access, and curl receiver
   interoperability into it. Its public surface is only configuration fixtures
   and a bounded test receiver; it exposes no native sender, listener used by
   production, route, background task, thread, DNS resolver, or routing API.
   Active daemon/CLI/graft/send-path crates have no dependency edge to it.
   Before moving code, capture a passing curl mTLS fixture proof using the
   existing certificate/fingerprint records; after the move, the identical
   fixture must pass from the new crate. This preserves verified provisioning
   evidence without preserving a native sender. Only after this before/after
   proof passes may active TLS code be deleted.
2. Delete `peer_resolution.rs`, literal-IP authority discovery, full peer-row
   scans, custom DNS threads, active `HttpsTransport`,
   `PinnedClientVerifier`, `TlsIdentity`, rustls composition, and the failing
   native TLS sender.
3. Retain AK.3 alias normalization, full-host persistence, and AK.4/AK.5's
   ordinary HTTP hostname resolution at connect time. Do not move any old
   sender/retry code into the interop crate merely to preserve it.
4. Delete obsolete TLS listener lifecycle/config validation from active daemon
   composition only after AK.4/AK.5 smoke has demonstrated the replacement.
   Retain the AK.4 `PeerHttpListenerSet` lifecycle unchanged. Preserve durable
   certificate/fingerprint rows only as interop provisioning data; do not let
   them influence active routing or retry decisions.
5. Finalize the active-transport documentation started in AK.4: mark ADR-034,
   ADR-040, and ADR-041 superseded by ADR-045; retain ADR-035 as the one
   ingress/router decision and amend only its obsolete TLS/worker language.
   Update `docs/adr/INDEX.md`, `docs/requirements.md`
   (`REQ-CORE-TRANSPORT-002`, `-002A`, `-002B`, `-002B1`, `-002C`, `-002D`,
   `-003`, `-003B`, `-004`, and `-005A`),
   `docs/{architecture,boundaries}.md`,
   `docs/atm-daemon/{architecture,boundaries,http-api,requirements}.md`,
   `docs/atm/{architecture,requirements}.md`, and
   `docs/peer-pair-smoke.md`. Documentation must distinguish the inactive
   `atm-peer-tls-interop` curl fixture from active production delivery. For
   `-002A/-002D`, AK.6 may update only supersession/status cross-references;
   AK.3 remains the exclusive owner of their alias semantics. For `-003/-003B`,
   AK.6 may update only supersession/status cross-references; AK.5 remains the
   exclusive owner of their resend-cache semantics. For `-002`, `-002B`,
   `-002B1`, `-002C`, `-004`, and `-005A`, AK.6 likewise updates only
   supersession/status cross-references; AK.4 remains the exclusive owner of
   active direct-delivery semantics.

## Explicit prohibitions

- No thread, worker, task, channel, DNS resolver, broad peer scan, native TLS
  sender, custom connection pool, or fallback transport may move into the new
  crate. The explicitly inventoried retained plain receiver is the only active
  listener execution.
- No active crate may depend on the interop crate, and curl evidence must not
  be represented as production native-send coverage.

## Required validation

- Source gate rejects legacy worker, scan, DNS-thread, and TLS symbols from
  active daemon/CLI/graft/send-path crates; `atm-peer-tls-interop` is the sole
  TLS owner.
- Dependency-graph check proves no active crate depends on
  `atm-peer-tls-interop`.
- TLS crate: provisioning/configuration round trip, accepted configured curl
  mTLS peer, and rejected unknown/mismatched client certificate pass; no test
  claims native ATM TLS send works.
- Integration: a legacy TLS configuration record cannot select a sender,
  listener, retry path, alias, or fallback after AK.6; an explicit configured
  IP alias still canonicalizes through AK.3 and reaches AK.4 HTTP only.
- Smoke: run `just smoke localhost`, `just smoke local-ip`, and isolated
  M4→M5 and M5→M4 `crosshost-send`, `crosshost-ack`, and
  `crosshost-curl-plain` lanes. Each direction proves remote read, ACK reply,
  full-host provenance, and exactly one receiver nudge. Curl mTLS fixture
  evidence is recorded separately and never substitutes for production send.
- `just lint` and `just test` pass.

## Dependencies

Before every AK.6 development/fix round, merge AK.5 into AK.6. Start AK.6 as
soon as AK.5 is pushed; do not wait for QA. AK.6 PR completion waits for AK.5
merge.
`must_follow` is required because deletion is safe only after AK.5's resend
path is proven. It is not parallel-safe because it removes its transport
dependencies.
