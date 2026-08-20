---
title: Phase AO Plan — Layered Optional mTLS Without Plain-Pipeline Drift
status: draft
branch: feature/pao-tls-replan
worktree: ../atm-core-worktrees/feature/pao-tls-replan
baseline: develop @ 2ed09a5b99eff11b7f64e2270cc1842e71a2f603
integration_branch: integrate/phase-ao2
archived_predecessor: docs/archive/phase-ao
---

# Phase AO — Layered Optional mTLS

## Decision Context

The prior AO plan is preserved at `docs/archive/phase-ao/` solely as an audit
record.  It must not be merged, repaired, reverted, or used as an
implementation base.  Its failure was architectural: it put adapter selection
and availability on the ordinary direct-peer startup path, so the working
plaintext pipeline changed even when TLS was disabled.

This replacement starts from `develop` and integrates through the new
`integrate/phase-ao2` line.  `integrate/phase-ao` remains reserved for the
archived predecessor.

The compatibility baseline is the existing direct-peer path:

```text
host-qualified CLI/graft write
  -> atm-http-runtime selected write transport
  -> DirectPeerTcpConnector
  -> direct-peer TCP listener
  -> canonical Axum router
  -> WriteRequest, persistence, post-write hook, and response
```

## Phase Contract

1. **mTLS remains the normal, fail-closed peer mode.** This retains the
   accepted policy in ADR-033/034/035 and
   `REQ-CORE-TRANSPORT-002B1` / `REQ-DAEMON-TRANSPORT-002B1`.
2. **`--peer-wire-security plaintext-test` remains an explicit, non-durable
   diagnostic and benchmark mode.** It is selected at daemon launch without
   rebuilding.  It is not inferred from certificate/configuration absence and
   it is never a TLS fallback.
3. **Selected plaintext is a compatibility oracle.** Its listener and client
   execute the pre-AO direct-peer path without constructing a TLS adapter,
   reading `PeerConfigStore`, reading a certificate/trust record, or branching
   on adapter availability.
4. **mTLS is an outer stream layer only.** It may authenticate/wrap accepted
   or connected TCP streams before the existing HTTP handling.  It cannot add
   a route, DTO, canonical write, persistence, acknowledgement, nudge,
   replay, or retry path.
5. **One shipped daemon supports both modes.** The normal launch argument,
   daemon-switch, smoke, and benchmark flows select the mode.  Cargo features,
   a benchmark-only daemon, and a test-only HTTP resource are forbidden.
6. **The frozen synchronous `crates/atm-daemon` is out of scope.** All daemon
   work belongs to the Tokio/Axum `atm-http-runtime` cutover line.

## Governing Record Audit

| Record | AO interpretation and required action |
| --- | --- |
| ADR-033 — HTTP Endpoint Contract | Retain one router and the existing `plaintext-test` diagnostic profile. AO.3 proves both modes use the same resource and application handlers. |
| ADR-034 — Minimal Cross-Host HTTPS Transport | Retain mTLS normal mode, exact hostname/pin/allowlist, no fallback, and pre-router rejection. ADR-047 must add the layered-stream and plaintext-compatibility invariant without weakening those controls. |
| ADR-035 — Canonical Write Ingress And Host Routing | Retain one `WriteRequest`, one handler, and typed untrusted plaintext-test provenance. TLS may not create a routing or ACK branch. |
| ADR-040 — Peer Authority Resolution | Retain hostname-plus-pin authorization; DNS/mDNS resolves a current endpoint and never creates a durable IP alias. |
| `REQ-CORE-TRANSPORT-002B1` / `REQ-DAEMON-TRANSPORT-002B1` | Retain default mTLS and explicit `plaintext-test`. AO.1 corrects their stale “superseded by ADR-047” traceability only when ADR-047 is added, and adds the no-plain-drift invariant. |
| `atm-storage::tls` and `PeerConfigStore` boundaries | Remain configuration/verification-only. AO.2 is the sole authority to authorize `peer-tls` as a transport consumer; neither storage surface gains socket I/O. |
| `atm-http-runtime` boundary | Retains the direct connector/listener, one router, and HTTP codec. It imports no Rustls, certificate policy, or storage trait; AO.3 may consume only an opaque stream seam. |

The missing ADR-047 cited by `docs/adr/INDEX.md` and
`docs/requirements.md` is a traceability defect. AO.1 owns creating that
record and repairing the references before TLS implementation begins.

## Layer Ownership And Mechanical Guards

| Layer | Owns | Must not own |
| --- | --- | --- |
| `atm-storage::tls` / `PeerConfigStore` | Certificate parsing, pin verification primitives, and durable interface/identity/trust configuration. | Socket I/O, handshake, delivery, routing, lifecycle, retries, or daemon startup policy. |
| `peer-tls` | Concrete Rustls/Tokio-Rustls values, identity loading after mTLS selection, hostname/pin/client-certificate verification, and inbound/outbound stream wrapping. | HTTP DTOs, router, mailbox, ACK, nudge, persistence, replay, retry, CLI, or daemon lifecycle. |
| `atm-core` | Typed peer-wire mode and error vocabulary only. | Rustls types, certificate parsing, config-store reads, HTTP routing, storage, or a transport trait-object extension point. |
| `atm-http-runtime` | Existing direct client/listener, one router/codec/application pipeline, and private generic HTTP-over-stream helpers. | Rustls/Tokio-Rustls, `PeerConfigStore`, certificate policy, second route/DTO, or feature-selected daemon. |
| `atm-daemon-bootstrap` | Parse the mode once and compose the mTLS stream provider only in the mTLS arm. | Handshake, certificate parsing, policy lookup, automatic mode inference, or mTLS-to-plain fallback. |
| `atm` / `atm-graft` | Submit the ordinary canonical daemon request and render typed outcomes. | Private TLS clients, certificate reads, mode-specific DTOs, or SQLite fallback. |

AO.1 and AO.3 must add architecture tests that fail if the plaintext arm
mentions `peer_tls`, Rustls, `PeerConfigStore`, adapter availability, or an
alternate connector/listener.  Completed guards must also prove one
HTTP route/resource, one `WriteRequest` encoding/response path, and no
mode-specific ACK, persistence, hook, retry, or benchmark process.

## Sprint Sequence

| Sprint | Authoritative doc | must_follow | Closure |
| --- | --- | --- | --- |
| AO.1 — Policy and plain baseline | [sprint-AO1](./sprint-AO1-policy-and-plain-baseline.md) | none | ADR/requirement reconciliation, mode contract, and a production-grade plain-pipeline oracle. |
| AO.2 — Isolated stream adapter | [sprint-AO2](./sprint-AO2-isolated-mtls-stream-adapter.md) | AO.1 development pushed and merged forward before every dev/fix round | A bounded `peer-tls` adapter with positive and negative stream evidence. |
| AO.3 — Runtime mode seam | [sprint-AO3](./sprint-AO3-runtime-mode-seam.md) | AO.2 development pushed and merged forward before every dev/fix round | One daemon build selects the original plaintext or mTLS stream establishment without application drift. |
| AO.4 — Operational and performance proof | [sprint-AO4](./sprint-AO4-operational-performance-proof.md) | AO.3 PR merged | Shipped-daemon plaintext/mTLS proof and compatible-baseline performance evidence. |

No pair is `parallel_safe`: every later sprint consumes the prior sprint's
public contract, boundary records, or live runtime seam.  A child may begin
planning after its parent is pushed, but before every development or fix round
it merges the parent integration tip; PR completion requires its parent PR to
merge first.

## Phase Acceptance

- ADR-047 and the cited requirements consistently define normal mTLS,
  explicit `plaintext-test`, one router, no fallback, and the plaintext
  compatibility invariant.
- Plaintext mode is proven structurally and behaviorally to retain the
  pre-AO direct connector/listener pipeline, including when TLS configuration
  is absent or invalid.
- mTLS rejects disabled/missing identity, invalid key, untrusted/expired
  client certificate, hostname mismatch, pin mismatch, deadline, and
  handshake failure before HTTP decode or router entry.
- Both modes use the same release daemon, route, request/response codec,
  canonical write, hook, acknowledgement, and persistence behavior.
- Selected `plaintext-test` throughput and latency match the compatible
  same-host/profile pre-AO baseline **regardless of mTLS code being present in
  the same shipped binary**; a material regression blocks phase closure. mTLS
  has its own recorded same-mode baseline after acceptance.
- M4, M5, and FastPC4 proof is required only in AO.4. An unavailable host is a
  retained blocked artifact, never a pass for physical-host evidence.

## AO.4 Physical-Host Evidence Ledger

| Campaign | Status | Evidence boundary | Recovery |
| --- | --- | --- | --- |
| M4 local `tcp` / `tcp-tls` | blocked | Both public target commands were invoked at `f40440d7bd370168ce41bf76ae486210df14b18d` and retained compact failure artifacts. `tcp` reports the stable `missing_compatible_plaintext_baseline` code (for example `20260820-220233.418717-local-tcp-plaintext-test-f1.json`); `tcp-tls` records that this active OS user already owns an ambient daemon, so it is not an isolated host. The live mTLS-negative lane was also invoked, but its preflight correctly rejected the mismatched active pair (candidate 1.4.4; CLI/daemon 1.4.3), retained at `site/reports/smoke/macos/rand-m4.local/20260820T222412780815Z-pid97059-crosshost-curl-tls/`; no M5 authentication or throughput result is claimed. | Establish a complete, provenance-qualified pre-AO plaintext baseline, switch a matched candidate CLI/daemon pair, and run the targets from a dedicated clean OS user; retain the resulting comparison artifacts. |
| M5 bidirectional `tcp` / `tcp-tls` | blocked | M5 was unavailable during the AO.4 implementation window; no send/read/ack/reply or benchmark result is claimed. | Restore M5 availability, run both directions/modes, and attach the retained evidence. |
| FastPC4 `tcp` / `tcp-tls` | blocked | FastPC4 was not available to this AO.4 session; no Windows benchmark result is claimed. | Restore approved remote access, run the Windows pair of campaigns, and attach the retained evidence. |

These entries are intentionally not performance outcomes. They prevent an
unavailable physical host from being mistaken for a successful proof while
leaving the functional and performance acceptance gates intact.

## Phase Non-Goals

- Repairing, merging, deleting, or reverting the archived predecessor.
- Editing, starting, or validating the frozen synchronous daemon.
- Certificate rotation/discovery, an outbox, replay/retry system, relay,
  controller, second router, or second message schema.
- Persisting IP aliases, reverse-DNS authorization, or implicit wire-mode
  selection from configuration state.
