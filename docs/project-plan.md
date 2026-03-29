# ATM CLI Project Plan

## 1. Goal

Implement a daemon-free ATM rewrite in this repo that preserves retained `send`, `read`, `log`, and `doctor` functionality.

The authoritative migration document is:
- `file-migration-plan.md`

This plan sequences the work. File-level migration decisions live in `file-migration-plan.md`.

## 2. Deliverables

- Rust workspace with `crates/atm-core` and `crates/atm`
- daemon-free implementation of `send`, `read`, `log`, and `doctor`
- preserved non-daemon send/read functionality from the current codebase
- explicit four-state workflow model with three display buckets
- structured errors with recovery guidance
- structured logs through `sc-observability`
- retained and new integration tests for the retained command surface

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
- log query/follow service over the ATM observability adapter
- doctor diagnostics service
- observability adapter
- error model

### 3.2 `crates/atm`

Implements:
- clap parser
- `send`
- `read`
- `log`
- `doctor`
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
- workflow state, display buckets, retained command surface, and observability boundary are consistent across all docs
- every retained or excluded source file needed for the retained commands is explicitly listed in `file-migration-plan.md`

### Phase A: `OBS-GAP-1`

Goal:
- verify and close the shared `sc-observability` API gap before ATM depends on it for `atm log` and `atm doctor`

Deliverables:
- ATM-side required capability list
- gap list against current `sc-observability`
- concrete API requests for `arch-obs`
- decision on ATM-owned adapter responsibilities versus shared observability responsibilities

Acceptance:
- shared plan exists for emit/query/follow/filter/health support
- no ATM-local ad hoc log query engine is needed

### Phase B: Core Skeleton

Create:
- workspace manifests
- `atm-core`
- `atm`
- placeholder module tree matching the architecture

Acceptance:
- workspace builds
- CLI help shows `send`, `read`, `log`, and `doctor`

### Phase C: Low-Level Reuse

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

### Phase D: Send Path

Port send command and support files:
- identity resolution
- file policy
- summary generation
- mailbox append
- command output
- observability emission

Acceptance:
- `atm send` feature set works without daemon support
- send JSON and human output match the documented contract

### Phase E: Read Path

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

### Phase F: Log Path

Port and redesign the log command:
- shared observability adapter
- log query/filter/tail behavior
- command output
- integration tests

Acceptance:
- `atm log` works through shared `sc-observability` APIs
- level and field filtering work
- tail mode works
- emit failures remain best-effort for mail commands

### Phase G: Doctor Path

Port and redesign the doctor command:
- local config/path checks
- hook identity checks
- mailbox readiness checks
- observability health and query-readiness checks
- command output

Acceptance:
- `atm doctor` works without daemon support
- doctor findings reflect the local daemon-free system
- observability readiness is visible in doctor output

### Phase H: Cleanup And Hardening

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
- Generic logging query/follow/filter behavior should live in `sc-observability` where possible, not in ATM-specific code.

## 6. Done Definition

The rewrite is ready when:
- `atm send` works without daemon support
- `atm read` works without daemon support
- `atm log` works through shared observability APIs
- `atm doctor` works as a local diagnostics command
- retained non-daemon functionality is preserved or intentionally documented as changed
- the file-by-file migration plan is complete enough to implement directly
- the retained command tests pass against the new crate layout
