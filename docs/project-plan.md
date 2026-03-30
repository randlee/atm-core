# ATM CLI Project Plan

## 1. Goal

Implement a daemon-free ATM rewrite in this repo that preserves retained `send`, `read`, `ack`, `clear`, `log`, and `doctor` functionality.

The authoritative migration document is:
- `file-migration-plan.md`

This plan sequences the work. File-level migration decisions live in `file-migration-plan.md`.

## 2. Deliverables

- Rust workspace with `crates/atm-core` and `crates/atm`
- daemon-free implementation of `send`, `read`, `ack`, `clear`, `log`, and `doctor`
- preserved non-daemon mail functionality from the current codebase
- explicit two-axis workflow model with three display buckets
- task-linked message metadata with mandatory ack behavior
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
- workflow axis model and transitions
- send, read, ack, and clear services
- log query/follow service over the injected observability port
- doctor diagnostics service
- observability event/query models and the observability port boundary
- error model

### 3.2 `crates/atm`

Implements:
- clap parser
- `send`
- `read`
- `ack`
- `clear`
- `log`
- `doctor`
- output formatting
- observability bootstrap
- concrete `sc-observability` port implementation and injection

## 4. Work Sequence

### Phase 0: Document Lock

Finish and freeze:
- `requirements.md`
- `architecture.md`
- `read-behavior.md`
- `file-migration-plan.md`

Acceptance:
- workflow axes, display buckets, retained command surface, and observability boundary are consistent across all docs
- every retained or excluded source file needed for the retained commands is explicitly listed in `file-migration-plan.md`

### Phase A: `OBS-GAP-1`

Goal:
- verify and close the shared `sc-observability` API gap before ATM depends on it for `atm log` and `atm doctor`

Deliverables:
- ATM-side required capability list
- gap list against current `sc-observability`
- concrete API requests for `arch-obs`
- decision on ATM-owned port-boundary responsibilities versus shared observability responsibilities

Acceptance:
- shared plan exists for emit/query/follow/filter/health support
- no ATM-local ad hoc log query engine is needed

### Phase B: Core Skeleton

| Sprint | Scope | Required outcome |
| --- | --- | --- |
| B.1 | CLI skeleton | `atm` exposes exactly `send`, `read`, `ack`, `clear`, `log`, `doctor` as clap subcommands and all CI gates pass |
| B.2 | Documentation gap closure | lock the remaining send/read/clear requirements and architecture details before Phase C begins |

Create:
- workspace manifests
- `atm-core`
- `atm`
- placeholder module tree matching the architecture

Acceptance:
- workspace builds
- CLI help shows `send`, `read`, `ack`, `clear`, `log`, and `doctor`
- B.1 and B.2 are both complete before Phase C starts
- requirements and architecture lock the message id, read dedupe, and clear
  override semantics needed for implementation

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
- ack-required and task-linked message creation
- command output
- observability emission

Acceptance:
- `atm send` feature set works without daemon support
- send JSON and human output match the documented contract

### Phase E: Read Path

Port read command and support files:
- workflow axis classification
- display bucket mapping
- selection modes
- seen-state behavior
- timeout waiting
- legal state transitions
- command output

Acceptance:
- `atm read` feature set works without daemon support
- workflow axes and display buckets match the requirements
- seen-state semantics match the documented contract

### Phase F: Ack And Clear Path

Port ack and clear command support files:
- acknowledgement transition handling
- reply emission
- clear eligibility computation
- clear dry-run reporting
- command output

Acceptance:
- `atm ack` feature set works without daemon support
- `atm clear` removes only clearable messages
- pending-ack messages remain visible until acknowledgement

### Phase G: Log Path

Port and redesign the log command:
- injected observability port usage
- log query/filter/tail behavior
- command output
- integration tests

Acceptance:
- `atm log` works through shared `sc-observability` APIs
- level and field filtering work
- tail mode works
- emit failures remain best-effort for mail commands

### Phase H: Doctor Path

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

### Phase I: Cleanup And Hardening

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
- Workflow-axis transitions must be enforced by code structure, not only by tests.
- Display bucket behavior must remain separate from the canonical two-axis workflow model.
- Task-linked mail must be ack-required from creation time.
- Generic logging query/follow/filter behavior should live in `sc-observability` where possible, not in ATM-specific code.

Cross-document invariants that must stay locked during implementation:
- `taskId` implies ack-required send behavior
- displayed messages always persist `read = true`
- pending-ack messages remain actionable until acknowledged
- `atm clear` never removes unread messages
- `atm clear` removes pending-ack messages only with explicit override
- `atm read --timeout` returns immediately when the requested selection is already non-empty

## 6. Done Definition

The rewrite is ready when:
- `atm send` works without daemon support
- `atm read` works without daemon support
- `atm ack` works without daemon support
- `atm clear` works without daemon support
- `atm log` works through shared observability APIs
- `atm doctor` works as a local diagnostics command
- retained non-daemon functionality is preserved or intentionally documented as changed
- task-linked mail remains pending until acknowledged unless the operator uses
  the explicit pending-ack clear override
- the file-by-file migration plan is complete enough to implement directly
- the retained command tests pass against the new crate layout

## 7. Documentation Review Checks

Before implementation starts, the docs should be reviewed with these checks:
- every retained or rejected source file referenced by the retained command surface appears in `file-migration-plan.md`
- `requirements.md`, `architecture.md`, and `read-behavior.md` agree on the two-axis model, three display buckets, and legal transitions
- `requirements.md`, `architecture.md`, and `read-behavior.md` agree on `--since`, `--since-last-seen`, `--no-since-last-seen`, `--no-update-seen`, and `--timeout`
- `requirements.md`, `architecture.md`, and `file-migration-plan.md` agree on the retained command set: `send`, `read`, `ack`, `clear`, `log`, `doctor`
