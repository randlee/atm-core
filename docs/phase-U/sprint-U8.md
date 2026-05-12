# Sprint U.8 — Shared Thin-Client ICD For CLI And Graft

```yaml
plan_type: sprint_plan
phase: U
sprint: "U.8"
worktree: /Users/randlee/Documents/github/atm-core-worktrees/feature/pU-u8-shared-thin-client-icd
branch: feature/pU-u8-shared-thin-client-icd
status: planned
estimated_scope: M
```

## Goal

Restack the abandoned earlier graft-client work so `atm-graft` uses the same
shared client/server ICD family as the CLI instead of a daemon-private API.

## Scope Summary

This sprint formalizes that `atm-graft` is a thin client. All request/response
traffic between `atm-graft` and the daemon must use shared `atm-core`
interfaces and the same ICD family the CLI already uses.

Lean-design rule:
- prefer the existing shared `ClientTransport` plus request/response DTOs
- do not add a graft-specific public trait family unless the shared boundary is
  provably insufficient
- do not preserve the current `Graft*` naming if a generic shared-ICD shape
  can carry the same behavior

## Governing Requirements

- `REQ-CORE-TRANSPORT-001`
- `REQ-CORE-TRANSPORT-002`
- `REQ-CORE-BOUNDARY-001`
- `REQ-P-CONTRACT-001`

## Governing ADRs

- `ADR-005` — host-scoped SQLite state root (`docs/adr/ADR-005-host-scoped-sqlite-state-root.md`)
- `ADR-ATM-CORE-001` (in `docs/atm-core/architecture.md`) — shared ATM
  protocol lives in `atm-core`
- `ADR-ATM-CORE-002` (in `docs/atm-core/architecture.md`) — shared transport/protocol contract

## Governing Boundaries

- `BOUNDARY-AtmProtocol`
- `BOUNDARY-ClientTransport`
- `BOUNDARY-ServerTransport`
- `BOUNDARY-RequestDispatcher`
- boundary lint must forbid `atm-graft` -> `atm-daemon`

## Prerequisites

- `U.1` through `U.7` are complete

## Hard Dependencies

- `U.1` — `metadata.atm` read-path removed before ICD restack
- `U.2` — one-message-identity ADR in place before graft protocol is restacked
- `U.5` — SQLite query cutover complete before ICD is treated as authoritative
- `U.7` — roster simplification complete; member addressing must be stable
  before the shared ICD family is finalized

## Dependency Notes

U.3 (thread/update hardening), U.4 (unified mutable state), and U.6
(provenance field reduction) are not blocking for U.8. The ICD restack does
not depend on internal mutable-state or provenance field decisions — it depends
only on the identity, query, and member-addressing surfaces above.

## Non-Goals

- daemon-owned client-specific runtime logic
- a client-specific daemon API
- graft-specific direct dependence on `atm-daemon`
- a new graft-only public trait layer when shared transport/protocol is enough

## Sub-Tasks

1. Rewrite the abandoned earlier graft-client intent around the shared protocol
   Development work:
   - define the `atm-graft` request/response needs in terms of the existing
     shared request/response envelopes and DTOs already present on `develop`
   - start from:
     - `crates/atm-core/src/boundary/mod.rs` `ClientTransport`
     - `crates/atm-core/src/protocol.rs` `RequestEnvelope` /
       `ResponseEnvelope`
     - `docs/atm-daemon/protocol-icd.md` current ICD inventory
     - current `develop @ b6506ef` references:
       - `crates/atm-core/src/graft.rs`
       - `crates/atm-core/src/protocol.rs`
       - `crates/atm/src/composition.rs`
   - forbid graft-private daemon API types
   - keep the client surface as simple as `command -> send` over the shared
     transport contract
   - if registration or advisory-delivery packets remain necessary, keep them
     in the same shared family but rename them generically rather than
     preserving `GraftRegister` / `GraftFetch` / `GraftDrain`
   Required tests:
   - protocol round-trip tests showing CLI and graft-shaped traffic share the
     same ICD family
   - reuse and extend:
     - `crates/atm-daemon/src/peer_transport.rs` round-trip protocol tests
     - `crates/atm-core/src/transport/testing.rs` `FakeClientTransport`
   Required doc or boundary updates:
   - update protocol/boundary docs to make the shared ICD rule explicit

2. Remove client-specific daemon contract drift
   Development work:
   - remove or rename any graft-specific protocol message family that would
     make the daemon API client-specific
   Required tests:
   - protocol inventory and boundary-lint coverage
   - explicit lint/boundary check proving `atm-graft` cannot reference
     `atm-daemon`
   Required doc or boundary updates:
   - update architecture/boundary docs to prohibit daemon-private graft APIs

## Acceptance Criteria

- `atm-graft` is modeled as a thin client using shared `atm-core` interfaces
- daemon request/response traffic for graft uses the same ICD family as CLI
- unary `send` / `read` / `ack` traffic shares the same request/response
  family as CLI
- any additive registration or advisory-delivery messages remain part of that
  same shared family and are renamed generically when reintroduced
- no graft-specific daemon API surface is introduced
- no extra graft-specific public trait family is introduced unless the shared
  transport/protocol contract proves insufficient
- historical `REQ-CORE-GRAFT-001` is explicitly retired or superseded in
  `docs/atm-core/requirements.md` by the shared `AtmProtocol` /
  `ClientTransport` family
- `atm-graft` has no Rust-crate dependency on `atm-daemon`, and that rule is
  lint-enforced rather than documented only
- U.8 owns protocol/DTO family ownership and rename planning only
- U.8 owns generic replacement of `GraftSessionId`
- actual removal or generic replacement of the following
  `docs/phase-U/removal-inventory.md` items is split across U.8 through U.10:
  - `GraftSessionPort`
  - `GraftSessionState`
  - `GraftSessionId`
  - `NudgeEvent`
  - `GraftNudgeFetchRequest`
  - `GraftNudgeDrainRequest`
- by end of U.8, the plan includes an explicit ownership matrix showing which
  sprint removes or generifies each item

## Required Validation

- `cargo test --workspace`
- `cargo xwin check --workspace --target x86_64-pc-windows-msvc`
- `cargo xwin check --workspace --tests --target x86_64-pc-windows-msvc`
- `just lint`
- `git diff --check`

## Required Document Updates

- `docs/plan-phase-U.md`
- `docs/project-plan.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-core/architecture.md`
- `docs/atm-core/requirements.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/protocol-icd.md`
- `docs/phase-U/removal-inventory.md`

## Risks And Watchouts

- do not add a second daemon API family for one client
- do not let “shared ICD” devolve into shared framing plus different payload
  contracts
- do not let thin-client needs force a dependency from `atm-graft` to
  `atm-daemon`
