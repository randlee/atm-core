---
title: AK.6 Preserve legacy TLS interop evidence
status: proposed
branch: feature/pak-s6-remove-legacy-peer-transport
worktree: ../atm-core-worktrees/feature/pak-s6-remove-legacy-peer-transport
target: integrate/phase-ak
recommended_agent: Cipher-311d
recommended_model: deep-reasoning
must_follow: Phase AI merge to develop
merge_gate: current integrate/phase-ak head
parallel_safe: true
---

# AK.6 — preserve legacy TLS interop evidence

## Closure

AK.6 begins from the pre-AK.2 `integrate/phase-ak` baseline, not from AK.2's
deletion branch. It preserves only TLS provisioning/configuration and curl
receiver interoperability in `crates/atm-peer-tls-interop`; it is not a native
ATM TLS sender and no active daemon, CLI, graft, or send-path crate may depend
on it. AK.2 owns deletion of all active custom TLS code.

AK.6 deliberately keeps baseline interop capture and its isolated fixture in
one sprint. Its pre-AK.2 baseline contains the source independently of AK.2's
deletion. The AK.6 branch must not merge active legacy TLS code back into the
post-AK.2 line; its only mergeable output is the isolated crate and its
interop-only documentation.

Cipher may start the isolated fixture and boundary record in parallel from the
pre-AK.2 baseline after the Phase AI entry gate. Final smoke and documentation
reconciliation waits only for a merge-forward of the current integration head;
AK.6 does not wait for AK.8 or AK.9. AK.10 must instead merge AK.6 and final
AK.9 before its completion, and AK.11 starts only after AK.6 merges.

The baseline capture boundary is explicit: copy only the verified provisioning
data and curl-mTLS receiver fixture inputs required by `TlsInteropConfig` and
`CurlMtlsReceiverFixture`. Do not copy `HttpsMessageTransport`,
`SharedHttpsTransport`, `HttpsTransport`, `TlsIdentity`, `PinnedClientVerifier`,
TLS client connection/open/deliver helpers, daemon composition slots, literal-IP
fallback, or DNS/thread helpers into the mergeable crate. AK.2 deletes those
active transport symbols; AK.3 separately removes resolver/authority code.

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
| `PeerHttpListenerSet`, `PeerHttpListener`, `PeerConnectionAdmission`, `route_peer_http_request`, `ActiveConnectionRegistry` | AK.4's new minimal production plain receiver. AK.6 must neither delete it nor add a replacement receiver/thread model. |
| `PeerHttpRuntimeConfig` | AK.4's retained immutable source-host snapshot. AK.6 must retain it without adding a TLS, resolver, or delivery capability. |
| `PeerWireSecurity`, `HttpsMessageTransport`, `SharedHttpsTransport`, `HttpsTransport`, `TlsIdentity`, `PinnedClientVerifier`, TLS handshake helpers | Pre-AK.2 baseline source. AK.2 deletes it from active code; AK.6 extracts only the fixed interop values/fixture. |

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
   Create `boundaries/atm-peer-tls-interop/tls-interop.toml` in this same PR
   with this complete `.just/lint_boundaries.py` contract:
   ```toml
   boundary_id = "BOUNDARY-PeerTlsInterop"
   owner_package = "atm-peer-tls-interop"
   owner_crate_path = "atm_peer_tls_interop"
   name = "PeerTlsInterop"

   [public]
   facade = "TlsInteropConfig"

   [implementation]
   type = "CurlMtlsReceiverFixture"
   module = "atm_peer_tls_interop"
   visibility = "public"
   constructor = "pub(crate)"

   [composition]
   roots = []

   [ownership]
   io_owns = ["tls_provisioning_configuration", "curl_mtls_fixture"]
   io_forbidden = ["production_delivery", "recipient_routing", "retry_state", "background_work"]

   [dependencies]
   allowed_dependents = []
   allowed_dependencies = ["atm-core", "atm-storage"]
   forbidden_edges = [
     "atm-daemon -> atm-peer-tls-interop",
     "atm -> atm-peer-tls-interop",
     "atm-graft -> atm-peer-tls-interop",
     "atm-runtime -> atm-peer-tls-interop",
     "atm-daemon-bootstrap -> atm-peer-tls-interop",
   ]

   [references]
   scope = "outside_owner_crate"
   forbidden = []

   [contracts]
   request_types = ["LocalCertificate", "TrustedPeer"]
   response_types = ["TlsInteropConfig", "CurlMtlsFixtureOutcome"]
   error_types = ["AtmError"]

   [testing]
   allowed_test_double_paths = []
   forbidden_test_bypasses = ["native_tls_sender", "production_listener"]

   [enforcement]
   lint_rules = ["LINT-BOUNDARY-PEER-TLS-INTEROP-EDGES"]
   review_gates = ["no_active_dependent", "no_native_sender", "no_production_listener"]

   [status]
   state = "planned"
   notes = ["Curl mTLS interop and provisioning preservation only."]
   ```
   Before moving code, capture a passing curl mTLS fixture proof using the
   baseline certificate/fingerprint records; after the move, the identical
   fixture must pass from the new crate. This preserves verified provisioning
   evidence without preserving a native sender or restoring active TLS code.
2. Retain AK.3 alias normalization, full-host persistence, and AK.4/AK.5/
   AK.8/AK.9's
   ordinary HTTP hostname resolution at connect time. Do not move any old
   sender/retry code into the interop crate merely to preserve it.
3. Do not restore obsolete TLS listener lifecycle/config validation to active
   daemon composition. Retain the AK.4 `PeerHttpListenerSet` lifecycle
   unchanged. Preserve durable
   certificate/fingerprint rows only as interop provisioning data; do not let
   them influence active routing or retry decisions.
   In this same PR, update `boundaries/atm-daemon/peer-http-adapter.toml`:
   remove the deleted `HttpsTransport`/`HttpsMessageTransport` ownership and
   references, retain the AK.4 `PeerHttpListenerSet` receiver boundary, and
   do not leave a concrete record describing removed TLS code.
4. Finalize the inactive-interop documentation started in AK.4: mark ADR-034,
   ADR-040, and ADR-041 superseded by ADR-047; retain ADR-035 as the one
   ingress/router decision and amend only its obsolete TLS/worker language.
   Update `docs/adr/INDEX.md`, `docs/requirements.md`
   (`REQ-CORE-TRANSPORT-002`, `-002A`, `-002B`, `-002B1`, `-002C`, `-002D`,
   `-003`, `-003B`, `-004`, and `-005A`),
   `docs/architecture.md`, `docs/atm-storage/boundaries.md`,
   `docs/atm-daemon/{architecture,boundaries,http-api,requirements}.md`,
   `docs/atm/{architecture,requirements}.md`, and
   `docs/peer-pair-smoke.md`. Documentation must distinguish the inactive
   `atm-peer-tls-interop` curl fixture from active production delivery. For
   `-002A/-002D`, AK.6 may update only supersession/status cross-references;
   AK.3 remains the exclusive owner of their alias semantics. For `-003/-003B`,
   AK.6 may update only supersession/status cross-references; AK.5 remains the
   exclusive owner of their resend-cache semantics. For `-002`, `-002B`,
   `-002B1`, `-002C`, `-004`, and `-005A`, AK.6 likewise updates only
   supersession/status cross-references; AK.4 remains the owner of the base
   direct-delivery semantics while AK.8/AK.9 exclusively own the one-request
   `messages[]` and atomic-confirmation amendments.
   For ADR-040, AK.6 updates only the supersession/status banner; AK.3
   remains the exclusive owner of alias/configuration content. For ADR-035,
   AK.6 updates only obsolete TLS/worker status wording; AK.3 owns its
   alias/admission language and AK.4 owns its one-receiver direct-delivery
   amendment. For ADR-034 and ADR-041, AK.6 updates only their
   supersession/status banners; ADR-047 is AK.4's direct-delivery decision.

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
- Dependency-graph check validates
  `boundaries/atm-peer-tls-interop/tls-interop.toml`, including all five
  active-crate `forbidden_edges`, and proves no active crate depends on
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

AK.6 may start immediately from the pre-AK.2 `integrate/phase-ak` baseline
after Phase AI merges to `develop`; it develops its isolated fixture, boundary
record, and resolver-cleanup ledger in a separate worktree without waiting for
AK.5, AK.8, or AK.9. Before final validation, any final fix round, or PR
completion, merge the current `integrate/phase-ak` head into AK.6 and rerun
its own validation. `merge_gate` means that current-head reconciliation, not
an AK.8/AK.9 feature dependency; the interop crate has no active send-path
edge.

The AK.6 worktree records the exact pre-AK.2 base commit at creation. It does
not merge AK.2 before capturing the interop fixture. Its required current-head
merge-forward happens only for final validation and PR completion; resolve that
merge by retaining the isolated crate, never by restoring active TLS code.
