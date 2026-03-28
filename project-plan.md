# ATM CLI Project Plan

## 1. Goal

Implement a daemon-free ATM rewrite in this repo that preserves retained `send` and `read` functionality.

The authoritative migration document is:
- `file-migration-plan.md`

This plan sequences the work. File-level migration decisions live in `file-migration-plan.md`.

## 2. Deliverables

- Rust workspace with `crates/atm-core` and `crates/atm`
- daemon-free implementation of `send` and `read`
- preserved non-daemon send/read functionality from the current codebase
- explicit four-state workflow model with three display buckets
- structured errors with recovery guidance
- structured logs through `sc-observability`
- retained integration tests for the send/read surface

## 3. Crates

### 3.1 `crates/atm-core`

Implements:
- path and home resolution
- config, bridge, and settings resolution
- hook identity resolution
- file policy
- mailbox I/O and origin merge
- workflow state model and transitions
- send and read services
- observability adapter
- error model

### 3.2 `crates/atm`

Implements:
- clap parser
- `send`
- `read`
- output formatting
- observability bootstrap

## 4. Work Sequence

### Phase 0: Document Lock

Finish and freeze:
- `requirements.md`
- `architecture.md`
- `read-behavior.md`
- `file-migration-plan.md`

Acceptance:
- workflow state, display buckets, and read selection semantics are consistent across all docs
- every retained or excluded source file needed for send/read is explicitly listed in `file-migration-plan.md`

### Phase A: Core Skeleton

Create:
- workspace manifests
- `atm-core`
- `atm`
- placeholder module tree matching the architecture

Acceptance:
- workspace builds
- CLI help shows `send` and `read`

### Phase B: Low-Level Reuse

Port retained foundational files first:
- home/path helpers
- config and bridge resolution
- address parsing
- text utilities
- schema types
- mailbox primitives
- hook identity

Acceptance:
- foundational unit tests pass
- no daemon references remain in foundational modules

### Phase C: Send Path

Port send command and support files:
- identity resolution
- file policy
- summary generation
- mailbox append
- command output
- observability events

Acceptance:
- `atm send` feature set works without daemon support
- send JSON and human output match the documented contract

### Phase D: Read Path

Port read command and support files:
- workflow state classification
- display bucket mapping
- selection modes
- seen-state behavior
- timeout waiting
- legal state transitions
- command output

Acceptance:
- `atm read` feature set works without daemon support
- workflow states and display buckets match the requirements
- seen-state semantics match the documented contract

### Phase E: Cleanup And Hardening

Delete:
- daemon-dependent crates and helpers not retained
- leftover imports from daemon-era surfaces

Add:
- integration tests
- snapshot tests
- documentation polish

Acceptance:
- implementation matches `requirements.md`, `architecture.md`, `read-behavior.md`, and `file-migration-plan.md`

## 5. Hard Rules

- Removing the daemon does not authorize removing retained mail functionality.
- File-level migration decisions must be explicit.
- Every retained useful source file must appear in `file-migration-plan.md`.
- Every reviewed non-retained file must also appear there with a `do not copy` decision.
- Workflow state transitions must be enforced by code structure, not only by tests.
- Display bucket behavior must remain separate from canonical workflow state.

## 6. Done Definition

The rewrite is ready when:
- `atm send` works without daemon support
- `atm read` works without daemon support
- retained non-daemon functionality is preserved or intentionally documented as changed
- the file-by-file migration plan is complete enough to implement directly
- the retained send/read tests pass against the new crate layout
