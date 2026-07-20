---
title: AI.3 unified error contract foundation
status: proposed
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
3. Move canonical code/template construction into one dependency-safe module
   and migrate `atm-storage`, `atm-core`, and protocol foundations.

## Acceptance criteria

- `AtmErrorKind` and `ProtocolErrorEnvelope` have no production definition or
  use.
- Protocol error round-trips preserve exactly code and message.
- No code-to-kind mapping remains in `protocol.rs` or another adapter.
- Existing error-code semantics are retained by focused tests.

## Required validation

`cargo test -p atm-storage -p atm-core`; serialization tests; `just lint`;
`just test`; error-contract architecture check.
