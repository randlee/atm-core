---
title: AI.3 unified error contract foundation
status: complete
branch: feature/pAI-s3-error-contract-foundation
worktree: ../atm-core-worktrees/feature/pAI-s3-error-contract-foundation
target: integrate/phase-AI
---

# AI.3 — unified error contract foundation

## Deliverables

1. Replace `AtmError { code, kind, message, recovery, source, backtrace }`
   with ADR-032's serializable `{ code, message }` contract.
2. Delete `AtmErrorKind`, recovery/source/backtrace accessors, and
   `ProtocolErrorEnvelope`; protocol responses carry the same `AtmError`.
   The deletion inventory is those three symbols and every code-to-kind mapping
   reachable from a protocol adapter.
3. Move canonical code/template construction into one dependency-safe module
   and migrate `atm-storage`, `atm-core`, and protocol foundations.

## Contract

```rust
pub struct AtmError {
    pub code: AtmErrorCode,
    pub message: String,
}
```

No alternate protocol error envelope, error kind hierarchy, or adapter-specific
error type is permitted after this sprint.

## Acceptance criteria

- `AtmErrorKind` and `ProtocolErrorEnvelope` have no production definition or
  use.
- Protocol error round-trips preserve exactly code and message.
- No code-to-kind mapping remains in `protocol.rs` or another adapter.
- Existing error-code semantics are retained by focused tests.

## Non-closure

AI.3 establishes the single type and protocol shape only. AI.4 migrates every
consumer and activates the repository-wide constructor gate.

## Required validation

`cargo test -p atm-storage -p atm-core`; serialization tests; `just lint`;
`just test`; error-contract architecture check.
