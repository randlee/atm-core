---
id: W.4
title: Peer Replay Recovery Text
status: planned
branch: TBD
worktree: TBD
---

# Sprint W.4 — Peer Replay Recovery Text

## Goals

- close the remaining uncovered replay-persistence recovery branches in
  `peer_transport`
- ensure peer replay persistence failures remain actionable when remote
  delivery outcome is unknown
- keep replay failure reporting on the shared ATM error and doctor surfaces;
  this sprint must not add a bespoke peer-reporting path
- preserve parity between cross-daemon peer transport and the other ATM
  interfaces so remote-delivery failure classes do not drift from the shared
  ATM error model

## Hard Dependencies

- no hard dependency on `W.2` or `W.3`
- must reuse the existing shared protocol/error contract and the persistent
  recovery-text rules document

## Required Work

- add missing replay-persistence recovery text on the listed peer branches
- verify shared protocol-envelope parity for peer-side failures
- collapse duplicate peer-specific error/reporting paths when they restate the
  same shared failure class
- make the independence/ordering rule explicit so implementation can proceed
  without waiting on unrelated same-host or SQLite work
- compare every touched peer replay failure class against the current `main`
  send-failure contract before finalizing any refactor

## Acceptance Criteria

- the sprint covers the exact uncovered replay-persistence branches listed in
  the current path inventory
- each uncovered branch gains `.with_recovery(...)` coverage or an explicit
  ruling that the lower layer already preserves the exact operator guidance
- the sprint applies the `V.4 Category Binding` table and satisfies the
  `Recovery Text Checklist` in `docs/atm-daemon/recovery-text-rules.md` for
  each covered branch; it does not invent a parallel policy
- the sprint verifies protocol-envelope parity so cross-daemon failures preserve
  the same ATM error code and recovery intent seen by other participants
- where peer transport currently duplicates shared error-mapping/reporting
  behavior for the same failure class, the sprint should collapse those paths
  onto one shared implementation
- the sprint identifies the shared ATM error/protocol/doctor functions that
  become the single source of truth for each touched replay-persistence
  failure class
- the sprint reconciles its local code inventory with the shared Phase `W`
  ATM code inventory in `docs/plan-phase-W.md`
- the sprint makes the CLI / doctor split verifiable in acceptance criteria:
  concise send failure at the command surface, deeper replay durability detail
  through the shared doctor/runtime-health surfaces
- req-qa can verify from the sprint doc that `W.4` is independently executable
  and does not hide an undocumented dependency on `W.2` or `W.3`

## Implementation Notes

Primary file in scope:
- `crates/atm-daemon/src/peer_transport.rs`

Shared parity file in scope:
- `crates/atm-core/src/protocol.rs`

Shared paths that must be reused or consolidated:
- `crates/atm-core/src/error.rs`
- `crates/atm-core/src/protocol.rs`
  - `ProtocolErrorEnvelope::{from_error,into_atm_error}`
- `crates/atm-core/src/doctor/mod.rs`
- `crates/atm/src/commands/doctor.rs`
- `crates/atm/src/output.rs`
- `crates/atm-daemon/src/runtime_health.rs`
- `crates/atm-daemon/src/runtime_status_cache.rs`

Current main CLI baseline to preserve:
- remote-delivery and replay-persistence failures must continue to surface
  through the shared ATM error surface rather than a peer-specific CLI
  formatter
- if the operator is told whether retry is safe or whether durable replay
  persisted, that guidance must stay aligned across CLI, cross-daemon
  consumers, and doctor follow-through

Stable ATM code inventory for this sprint:
- replay store missing during outcome-unknown persistence:
  - final operator-facing ATM code remains
    `AtmErrorCode::RemoteDeliveryOutcomeUnknown`
  - lower-layer persistence prerequisite failure remains
    `AtmErrorCode::DaemonUnavailable` as the wrapped source
- peer endpoint missing during outcome-unknown persistence:
  - final operator-facing ATM code remains
    `AtmErrorCode::RemoteDeliveryOutcomeUnknown`
  - lower-layer persistence prerequisite failure remains
    `AtmErrorCode::DaemonUnavailable` as the wrapped source
- retry-budget to expiry conversion failure during replay persistence:
  - final operator-facing ATM code remains
    `AtmErrorCode::RemoteDeliveryOutcomeUnknown`
  - lower-layer persistence prerequisite failure remains
    `AtmErrorCode::DaemonUnavailable` as the wrapped source
- no new `AtmErrorKind` or `AtmErrorCode` variants are planned for `W.4`

Targeted functions:
- `PeerClientTransport::persist_replay_request(...)`
- `PeerClientTransport::persist_outcome_unknown_request(...)`

Current branch-level insertion candidates:
- replay store missing branch
- peer endpoint missing branch
- retry-budget to expiry conversion failure branch

Current path inventory:
- `crates/atm-core/src/error.rs`
  - remote-delivery-outcome-unknown and daemon-unavailable constructors/stable
    code bindings reused by replay-persistence failures
- `crates/atm-daemon/src/peer_transport.rs`
  - `PeerClientTransport::persist_replay_request(...)`
    - replay store missing
    - peer endpoint missing
    - retry-budget to expiry conversion failure
  - `PeerClientTransport::persist_outcome_unknown_request(...)`
    - replay metadata unsupported branch
    - durable replay persistence follow-through
  - `PeerTransportRuntime::persist_replay_request(...)`
    - runtime wrapper propagation path
- `crates/atm-core/src/protocol.rs`
  - `ProtocolErrorEnvelope::{from_error,into_atm_error}` propagation for remote
    delivery / replay persistence failures

Supporting verification paths:
- replay persistence tests in `crates/atm-daemon/src/peer_transport.rs`
- any doctor/runtime-health text that refers to remote replay durability

Critical issue classes covered directly by this sprint:
- ATM send failure where remote delivery outcome is unknown
- replay persistence failure that would otherwise leave the operator unsure
  whether retry is safe

CLI / doctor split:
- ATM CLI must keep the immediate send failure concise and tell the operator
  whether durable replay did or did not persist
- cross-daemon consumers must receive the same ATM error code and aligned
  recovery intent for the same remote-delivery / replay-persistence failure
  class
- `atm doctor` should remain able to explain replay-store configuration and
  retained pending replay state if that information is already available
- this sprint is primarily a regression-closure check on uncovered recovery
  branches, not a redesign of how send failure is reported

Cross-sprint dependency:
- if `W.3` is running in parallel, any `crates/atm-core/src/protocol.rs`
  change here must stay limited to peer replay / remote-delivery envelope
  parity and be merge-forwarded before either branch pushes a final head

## Out of Scope

- broad peer transport redesign
- daemon-client startup tracing
- SQLite subsystem instrumentation

## Required Validation

Plan-auditable now:
- explicit independence from `W.2` and `W.3`
- explicit shared-protocol parity ownership
- explicit duplicate-path collapse responsibility

Implementation validation later:
- parity proof that equivalent peer replay failure classes preserve the same
  ATM code/recovery intent through protocol envelopes
- test coverage for the listed replay-persistence branches
- proof that duplicate peer-specific mapping/reporting logic was collapsed
  onto shared ATM error / protocol / doctor implementations
