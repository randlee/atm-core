---
title: AK.6 Remove legacy peer transport machinery
status: proposed
target: integrate/phase-ak
recommended_agent: arch-ctm
recommended_model: deep-reasoning
must_follow: AK.5
parallel_safe: false
---

# AK.6 — remove legacy peer transport machinery

## Closure

After AK.4/AK.5 prove the replacement path, delete legacy scans, DNS threads,
and active custom TLS. Preserve only provisioning/configuration and curl
receiver interoperability in an isolated unused TLS crate; it is not a native
ATM TLS sender.

## Deliverables

1. Move verified TLS provisioning, certificate/fingerprint configuration/data
   access, and curl receiver interoperability into an isolated crate with no
   dependency from active daemon/CLI/graft paths. It exposes no native sender.
2. Delete `peer_resolution.rs`, literal-IP authority discovery, full peer-row
   scans, custom DNS threads, active `HttpsTransport`,
   `PinnedClientVerifier`, `TlsIdentity`, rustls composition, and the failing
   native TLS sender.
3. Retain AK.3 alias normalization, full-host persistence, and ordinary HTTP
   hostname resolution at connect time.
4. Update ADR-034/035, requirements, architecture/boundaries, OpenAPI if
   affected, and smoke documentation.

## Required validation

- Source gate rejects legacy worker, scan, DNS-thread, and TLS symbols from
  active daemon/CLI/graft crates; the isolated TLS crate is the sole TLS owner.
- TLS crate: provisioning/configuration round trip and curl mTLS receiver
  interop pass; no test claims native ATM TLS send works.
- M4↔M5 smoke proves curl and production send/read/ACK/nudge.
- `just lint` and `just test` pass.

## Dependencies

Before every AK.6 development/fix round, merge AK.5 into AK.6. Start AK.6 as
soon as AK.5 is pushed; do not wait for QA. AK.6 PR completion waits for AK.5
merge.
`must_follow` is required because deletion is safe only after AK.5's resend
path is proven. It is not parallel-safe because it removes its transport
dependencies.
