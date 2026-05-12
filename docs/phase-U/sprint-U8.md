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

- `docs/adr/ADR-005-host-scoped-sqlite-state-root.md`
- shared thin-client protocol ownership ADRs in `docs/atm-core/architecture.md`

## Governing Boundaries

- `BOUNDARY-AtmProtocol`
- `BOUNDARY-ClientTransport`
- `BOUNDARY-ServerTransport`
- `BOUNDARY-RequestDispatcher`
- boundary lint must forbid `atm-graft` -> `atm-daemon`

## Prerequisites

- `U.1` through `U.7` are complete

## Hard Dependencies

- `U.1`
- `U.2`
- `U.5`

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
- `atm-graft` has no Rust-crate dependency on `atm-daemon`, and that rule is
  lint-enforced rather than documented only

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
- `docs/atm-daemon/protocol-icd.md`
- `docs/phase-U/removal-inventory.md`

## Risks And Watchouts

- do not add a second daemon API family for one client
- do not let “shared ICD” devolve into shared framing plus different payload
  contracts
- do not let thin-client needs force a dependency from `atm-graft` to
  `atm-daemon`
