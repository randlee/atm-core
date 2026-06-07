# AC.5 RPC Envelope And Domain Type Unification

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.5
worktree: ../atm-core-worktrees/feature/pAC-s5-rpc-envelope-and-domain-type-unification
branch: feature/pAC-s5-rpc-envelope-and-domain-type-unification
status: planned
estimated_scope: large
```

## Goal

Replace per-message transport DTO proliferation with one generic RPC envelope
that carries canonical domain bodies.

## Scope Summary

This sprint is the RPC/domain reset line. It does not redefine the storage
contract. It makes the transport layer consume the same canonical structs that
the storage layer now uses.

## Governing Sources

- `docs/plan-phase-AC.md`
- `docs/phase-AC/sprint-AC1.md`
- current RPC/protocol code in `atm-core`, `atm-daemon`, and `atm-daemon-client`

## Prerequisites

- `AC.1`
- `AC.4`

## Out Of Scope

- no backend extraction work
- no transport-protocol redesign beyond envelope/body unification

## Deliverables

- the transport layer uses one generic envelope:

  ```rust
  pub struct RpcEnvelope {
      pub header: RpcHeader,
      pub body: bytes::Bytes,
  }
  ```

- message, roster, and task bodies decode into the canonical shared domain structs from `atm-storage`
- per-message transport clones are deleted unless a real semantic difference remains
- the same canonical `Message` struct is passed over RPC and into storage

## Acceptance Criteria

- no new transport-only message clones remain where the shared canonical struct is sufficient
- RPC envelope headers carry transport concerns only
- RPC bodies decode into shared domain structs rather than backend- or transport-specific clones

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `git diff --check`
- `rg -n ".*Request|.*Response" crates/atm-core crates/atm-daemon crates/atm-daemon-client -S`

## Required Document Updates

- `docs/phase-AC/sprint-AC5.md`
- `docs/phase-AC/readiness.md`
- `docs/project-plan.md`
- protocol and architecture docs that describe transport/body shapes

## Risks And Watchouts

- if the generic envelope keeps transport-specific body clones, the type explosion will survive under a new name
- if transport metadata is pushed back into the shared domain structs, the layering will invert again
