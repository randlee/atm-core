---
title: AO.3 — Runtime Peer-Wire Mode Seam
status: planned
branch: feature/pao-s3-runtime-peer-wire-mode
target: integrate/phase-ao2
worktree: ../atm-core-worktrees/feature/pao-s3-runtime-peer-wire-mode
external_blockers: []
---

# AO.3 — Runtime Peer-Wire Mode Seam

**recommended_agent:** arch-ctm/deep-reasoning.
**must_follow:** AO.2 development pushed. Merge AO.2's integration tip before
every AO.3 development or fix round; AO.2 PR must merge before AO.3 PR
completion.
**parallel_safe:** none. AO.3 owns the live bootstrap/runtime selection seam.
**unblocks:** AO.4.

**traceability:** ADR-033, ADR-034, ADR-035, ADR-047;
`REQ-CORE-TRANSPORT-001`, `-002B1`; `REQ-DAEMON-TRANSPORT-002B1`;
`boundaries/atm-http-runtime/http-runtime.toml` and
`boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml`.

## Goal

Select mTLS or explicit plaintext-test at one daemon-launch seam while keeping
both modes on one HTTP application pipeline.

## Scope Summary

Tokio/Axum bootstrap and runtime wiring, doctor/observability projection, and
mode-seam regression guards. No new delivery state or application resource.

## Governing Requirements

`REQ-CORE-TRANSPORT-001`, `-002B1`, and
`REQ-DAEMON-TRANSPORT-002B1` as reconciled by AO.1.

## Governing ADRs

ADR-033, ADR-034, ADR-035, and ADR-047.

## Governing Boundaries

`boundaries/atm-http-runtime/http-runtime.toml`,
`boundaries/atm-daemon-bootstrap/replacement-bootstrap.toml`, and AO.2's
`peer-tls` boundary manifest.

## Prerequisites

AO.2's approved concrete stream facade and guards are merged forward into this
worktree before every development or fix round.

## Hard Dependencies

AO.2 development pushed; AO.2 PR merged before AO.3 PR completion.

## Deliverables

1. Parse `--peer-wire-security` exactly once in Tokio/Axum daemon bootstrap.
   `Mtls` composes the AO.2 opaque stream adapter; `PlaintextTest` invokes the
   preserved `DirectPeerTcpConfig::standard`, `DirectPeerTcpConnector`, and
   direct listener. A mode error or mTLS setup/handshake failure never enters
   the plaintext arm.
2. Keep one `HttpRuntime` application path. Both modes use the current HTTP
   method/path, codec, `WriteRequest`, `ApiRouter`, dispatcher, persistence,
   post-write router, response envelope, request deadline, and lifecycle
   ownership. Only accepted/connected stream establishment differs.
3. Expose selected mode and mTLS readiness in doctor and retained observability
   without key bytes, certificate contents, pins, or raw trust records.
4. Extend AO.1's structural guard over complete wiring. It must reject TLS
   construction/config access in the plaintext arm; reject mTLS-to-plaintext
   fallback; and reject a second route, DTO, ACK sender, persistence/hook
   branch, or benchmark process.
5. Add release-interface tests for both modes. Plaintext tests run with invalid
   TLS state and prove the original pipeline; mTLS tests prove a configured
   peer succeeds and every failure stays pre-router with no downgrade.

## Acceptance Criteria

- One built daemon changes mode only by normal launch argument plus restart.
- `plaintext-test` reaches the pre-AO connector/listener path exactly and does
  not depend on TLS configuration.
- mTLS supplies only an authenticated stream to the canonical application path
  and rejects an invalid peer before HTTP decode.
- The frozen synchronous daemon is neither imported, built as a target of the
  feature, nor started by tests.

## Required Validation

- `cargo test -p atm-daemon-bootstrap -p atm-http-runtime -p peer-tls`
- `cargo test -p atm-architecture --test boundary_enforcement`
- `just lint`
- `just test`
- `just smoke` in both selected modes on a disposable local configuration.

## Required Document Updates

- Bootstrap/daemon-switch operations, doctor schema, observability event
  reference, boundary manifests, ADR-047 implementation evidence, and smoke
  procedure.

## Split Recommendation

Do not split: bootstrap selection, inbound/outbound stream wiring, and the
one-route guard form one correctness boundary. Splitting them could ship a
mode that is configured but not consistently enforced.

## Error Inventory

| Failure mode | Stable code ownership | Required recovery |
| --- | --- | --- |
| Invalid mode parsing or forbidden mode source | Reuse AO.1's central mode codes. | Correct the daemon launch argument; do not set environment fallbacks. |
| mTLS configuration/adapter construction failure | Reuse the AO.2 central configuration/certificate code. | Correct the registered interface/identity/trust records and restart in mTLS mode. |
| mTLS connection, handshake, or peer-authentication failure | Reuse the AO.2 central transport/authentication code. | Repair the named peer configuration or network path; the daemon must not switch to plaintext. |
| Plaintext-test request presented as authenticated | Use the AO.1 policy/authentication code. | Restart in mTLS mode for authenticated peer delivery. |

Doctor, CLI, and HTTP error translation must preserve the central code and
recovery classification. They may add mode/context, never convert it to a bare
string or expose key/certificate material.

## Paths To Delete

None. Do not modify or revive the frozen synchronous daemon.

## Non-Goals

AO.3 does not add delivery state, replay, retry, a remote query API,
certificate rotation/discovery, host/IP alias persistence, or a new controller.

## Risks And Watchouts

The key risk is an optional-adapter branch leaking into plaintext startup.
The structural guard and invalid-TLS plaintext tests are mandatory regression
oracles, not substitute test coverage.
