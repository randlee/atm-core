---
title: AO.1 — Peer-Wire Policy and Plain-Pipeline Baseline
status: complete
branch: feature/pao-s1-peer-wire-policy
target: integrate/phase-ao2
worktree: ../atm-core-worktrees/feature/pao-s1-peer-wire-policy
external_blockers: []
---

# AO.1 — Peer-Wire Policy and Plain-Pipeline Baseline

**recommended_agent:** arch-ctm/deep-reasoning.
**must_follow:** none. This establishes the contract later AO sprints consume.
**parallel_safe:** none. It owns the public mode policy, ADR/requirement
reconciliation, and source-level baseline guards.
**unblocks:** AO.2.

**traceability:** ADR-033, ADR-034, ADR-035, ADR-040;
`REQ-CORE-TRANSPORT-001`, `-002B`, `-002B1`, `-002C`;
`REQ-DAEMON-TRANSPORT-002A`, `-002B`, `-002B1`, `-002C`; and
`boundaries/atm-http-runtime/http-runtime.toml`.

## Goal

Make the peer-wire policy unambiguous and establish executable proof that
explicit plaintext diagnostic mode preserves the current direct-peer pipeline.

## Scope Summary

Documentation, typed mode vocabulary, characterization tests, and architecture
guards only.  TLS stream implementation and runtime wiring are excluded.

## Governing Requirements

`REQ-CORE-TRANSPORT-001`, `-002B`, `-002B1`, and `-002C`; and
`REQ-DAEMON-TRANSPORT-002A`, `-002B`, `-002B1`, and `-002C`.

## Governing ADRs

ADR-033, ADR-034, ADR-035, and ADR-040. AO.1 creates ADR-047 as the missing
layered-mode decision that reconciles their transport traceability.

## Governing Boundaries

`boundaries/atm-http-runtime/http-runtime.toml` and the core typed-error and
mode boundary records added by this sprint.

## Prerequisites

`develop` remains the unchanged direct-peer compatibility baseline. The
synchronous daemon is frozen and unavailable to this sprint.

## Hard Dependencies

None within AO. ADR-047 and amended requirements must be accepted before AO.2
can introduce an mTLS dependency.

## Deliverables

1. Add ADR-047, repair `docs/adr/INDEX.md`, and repair the requirements'
   dangling ADR-047 references. The new ADR must retain default mTLS and the
   explicit, non-durable `plaintext-test` diagnostic/benchmark mode. It must
   state that plaintext mode executes the preserved direct-peer pipeline and
   that neither absent TLS configuration nor mTLS failure changes the selected
   mode.
2. Specify the mode vocabulary at the transport-neutral boundary without
   importing TLS types. The authoritative interface is equivalent to:

   ```rust
   pub enum PeerWireSecurity { Mtls, PlaintextTest }

   pub struct PeerWireMode {
       pub security: PeerWireSecurity,
   }
   ```

   `Mtls` is the default. Parsing is limited to the daemon launch argument;
   environment configuration and adapter-availability inference are forbidden.
3. Add a characterization suite for the present plaintext direct-peer path:
   bind/readiness, host-qualified outbound selection, canonical
   `WriteRequest` encoding, router provenance, durable write, hook warning,
   acknowledgement, duplicate behavior, typed connection failure, and
   shutdown/readiness behavior.
4. Add source/architecture guards proving the plaintext selection reaches
   `DirectPeerTcpConfig::standard`, `DirectPeerTcpConnector`, the direct-peer
   listener, and the ordinary router without mentioning `peer_tls`, Rustls,
   `PeerConfigStore`, certificate/trust configuration, or adapter availability.
   The same guards must reject a second HTTP route, DTO, ACK sender,
   persistence path, hook path, or benchmark-only daemon.

## Acceptance Criteria

- The accepted records consistently distinguish normal mTLS from explicitly
  selected untrusted `plaintext-test`; no requirement or ADR calls the latter
  production authentication.
- The missing ADR-047 reference is no longer dangling, and its decision is
  cited by every amended requirement.
- All plaintext characterization tests pass unchanged with intentionally
  absent, invalid, or conflicting TLS configuration.
- The source guards fail on any TLS/configuration access or adapter branch in
  plaintext startup, listener, or connector selection.

## Required Validation

- `cargo test -p atm-core -p atm-http-runtime -p atm-daemon-bootstrap`
- `cargo test -p atm-architecture --test boundary_enforcement`
- `just lint`
- `just test`

## Required Document Updates

- ADR-047, ADR index, core/daemon requirements, boundary manifests, daemon
  mode-operation documentation, and smoke/benchmark documentation.

## Split Recommendation

Do not split: policy reconciliation and the plain-pipeline oracle must land
together, otherwise either record could falsely certify the other.

## Error Inventory

| Failure mode | Stable code ownership | Required recovery |
| --- | --- | --- |
| Unknown `--peer-wire-security` value | AO.1 adds a central `ATM_PEER_WIRE_MODE_INVALID` registry code. | Show accepted values, correct the launch argument, then restart the daemon. |
| A durable/environment setting attempts to select the mode | AO.1 adds `ATM_PEER_WIRE_MODE_SOURCE_FORBIDDEN`. | Remove the unsupported source and use the documented daemon launch argument. |
| Plaintext-test is selected for a claim requiring peer authentication | Reuse or add a fail-closed transport-policy code in the central registry. | Restart in mTLS mode and verify the configured identity, trust, and enabled interface. |

Every code must retain the existing structured `AtmError` envelope, recovery
class, doctor/log representation, and documentation entry.  AO.1 may not use
opaque strings or a local error enum.

## Paths To Delete

None. The archived AO documents remain audit evidence.

## Non-Goals

AO.1 adds no Rustls dependency, listener wrapping, mTLS client, or production
runtime wiring. It closes the policy and regression oracle only.

## Risks And Watchouts

The existing ADR/requirement wording treats plaintext as smoke-only. AO.1 must
retain that security boundary while making its preserved-pipeline guarantee
explicit; it must not quietly promote plaintext to normal authenticated use.
