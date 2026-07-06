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
- `crates/atm-core/src/protocol.rs`
- `crates/atm-core/src/lib.rs`
- `crates/atm-daemon-client/src/wire.rs`
- `crates/atm/src/composition.rs`
- `boundaries/atm-daemon-client/rpc-envelope.toml`
- `docs/atm-core/requirements.md`
- `docs/atm-core/boundaries.md`
- `docs/atm-daemon/protocol-icd.md`

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
- advisory register/unregister/fetch/drain/stream variants from
  `crates/atm-core/src/protocol.rs`
- the production `AdvisorySessionPort` implementation plus
  `register_graft_session(...)`, `unregister_graft_session(...)`,
  `fetch_graft_nudges(...)`, and `drain_graft_nudges(...)` from
  `crates/atm/src/composition.rs`
- advisory register/unregister/fetch/drain/stream packet kinds from
  `crates/atm-daemon-client/src/wire.rs`
- `atm_graft::transport::open_advisory_stream` from
  `boundaries/atm-daemon-client/rpc-envelope.toml`
- advisory register/unregister/fetch/drain/stream rows from
  `docs/atm-daemon/protocol-icd.md`
- shared advisory packet-family claims from `docs/atm-core/requirements.md`

## Deliverables

- the shared ATM boundary no longer models daemon-owned graft session lifecycle
- `crates/atm-core/src/protocol.rs` no longer reserves advisory register,
  unregister, fetch, drain, or stream request/response families in the shared
  envelope model
- the CLI composition layer no longer carries a production
  `AdvisorySessionPort` implementation or graft-session helper methods just to
  satisfy the leaked shared boundary
- the accepted daemon wire registry no longer carries graft-only advisory
  packet families
- the rpc-envelope governance record no longer lists graft advisory streaming
  as an accepted transport composition root
- shared boundary docs and daemon protocol docs no longer claim daemon-owned
  graft advisory stream/session protocol as the accepted design
- shared requirements docs no longer reserve graft session/stream concepts in
  `atm-core`

## This Sprint Does Not Close

- daemon runtime deletion
- `atm-graft` implementation rewrite
- smoke/readiness closeout

## Acceptance Criteria

- `atm-core` exports no shared graft advisory session/stream DTO surface, and
  `crates/atm-core/src/protocol.rs` exports no advisory register/unregister/
  fetch/drain/stream envelope variants
- `crates/atm/src/composition.rs` no longer exposes the production
  `AdvisorySessionPort` implementation or the graft-session helper methods
  required by that leaked shared contract
- `atm-daemon-client` exports no graft-only advisory packet kinds
- `boundaries/atm-daemon-client/rpc-envelope.toml` no longer governs a graft
  advisory-stream composition root
- `docs/atm-daemon/protocol-icd.md` and `docs/atm-core/boundaries.md`
  describe the reset boundary rather than daemon-owned graft session queues
- `docs/atm-core/requirements.md` no longer locks the shared boundary into
  graft session/stream packet families
- no remaining accepted boundary doc tells implementers to add graft-specific
  stream methods back to `RequestDispatcher`

## Required Validation

- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- `python3 .just/run_lint.py all`
- `rg -n "dispatch_advisory_stream|AdvisoryStreamSink|AdvisorySessionPort|Advisory(Register|Unregister|Fetch|Drain|Stream)|open_advisory_stream" crates/atm-core crates/atm-daemon-client crates/atm boundaries/atm-daemon-client/rpc-envelope.toml`
- `git diff --check`
