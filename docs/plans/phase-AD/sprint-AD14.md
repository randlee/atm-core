---
id: AD.14
title: Shared Graft Boundary Surface Reset
status: planned
branch: feature/pAD-s14-shared-graft-boundary-surface-reset
worktree: ../atm-core-worktrees/feature/pAD-s14-shared-graft-boundary-surface-reset
target: integrate/phase-AD
---

# Sprint AD.14 — Shared Graft Boundary Surface Reset

## Goal

- remove graft-only session and stream protocol concepts from the shared
  `atm-core` and `atm-daemon-client` boundary surface

## Hard Dependencies

- `AD.13` complete
- `docs/plans/phase-AD/plan-phase-AD.md`
- `docs/plans/phase-AD/violation-inventory.md`

## Exact Targets

- `crates/atm-core/src/boundary/mod.rs`
- `crates/atm-core/src/graft.rs`
- `crates/atm-core/src/lib.rs`
- `crates/atm-daemon-client/src/wire.rs`
- `docs/atm-core/requirements.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/protocol-icd.md`
- `docs/atm-daemon/requirements.md`
- `docs/atm-daemon/architecture.md`

## Interfaces To Add Or Modify

The accepted shared dispatcher contract after this sprint is:

```rust
pub trait RequestDispatcher: sealed::Sealed + Send + Sync {
    fn dispatch(&self, request: RequestEnvelope) -> Result<ResponseEnvelope, AtmError>;
}
```

The accepted shared graft contract after this sprint is:

```rust
pub trait AtmGraftClient: Send + Sync {
    fn send_message(&self, request: SendRequest) -> Result<SendOutcome, AtmError>;
    fn read_message(&self, query: ReadQuery) -> Result<ReadOutcome, AtmError>;
    fn acknowledge_message(&self, request: AckRequest) -> Result<AckOutcome, AtmError>;
}
```

## Paths To Delete

- `RequestDispatcher::dispatch_advisory_stream(...)`
- `AdvisoryStreamSink`
- `AdvisorySessionPort`
- `AdvisorySessionId`
- `AdvisorySession`
- `AdvisorySessionState`
- `AdvisorySessionRegistrationRequest`
- `AdvisorySessionRegistrationResponse`
- `AdvisorySessionUnregistrationRequest`
- `AdvisorySessionUnregistrationResponse`
- `AdvisoryFetchRequest`
- `AdvisoryFetchResponse`
- `AdvisoryDrainRequest`
- `AdvisoryDrainResponse`
- `AdvisoryStreamRequest`
- `AdvisoryStreamResponse`
- advisory register/unregister/fetch/drain/stream packet kinds from
  `crates/atm-daemon-client/src/wire.rs`
- advisory register/unregister/fetch/drain/stream rows from
  `docs/atm-daemon/protocol-icd.md`
- shared advisory packet-family claims from `docs/atm-core/requirements.md`
- advisory packet-family and queue-ownership claims from
  `docs/atm-daemon/architecture.md`

## Deliverables

- the shared ATM boundary no longer models daemon-owned graft session lifecycle
- the accepted daemon wire registry no longer carries graft-only advisory
  packet families
- shared boundary docs and daemon protocol docs no longer claim daemon-owned
  graft advisory stream/session protocol as the accepted design
- shared requirements docs no longer reserve graft session/stream concepts in
  `atm-core` or `atm-daemon`

## This Sprint Does Not Close

- daemon runtime deletion
- `atm-graft` implementation rewrite
- smoke/readiness closeout

## Acceptance Criteria

- `atm-core` exports no shared graft advisory session/stream DTO surface
- `atm-daemon-client` exports no graft-only advisory packet kinds
- `docs/atm-daemon/protocol-icd.md`, `docs/atm-daemon/requirements.md`, and
  `docs/atm-core/boundaries.md` describe the reset boundary rather than
  daemon-owned graft session queues
- `docs/atm-core/requirements.md` and `docs/atm-daemon/architecture.md` no
  longer lock the shared boundary into graft session/stream packet families
- no remaining accepted boundary doc tells implementers to add graft-specific
  stream methods back to `RequestDispatcher`

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `rg -n "dispatch_advisory_stream|AdvisoryStreamSink|AdvisorySessionPort|Advisory(Register|Unregister|Fetch|Drain|Stream)" crates/atm-core crates/atm-daemon-client`
- `git diff --check`
