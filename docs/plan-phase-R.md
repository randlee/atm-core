# Phase R Task List

## 1. Goal

Repeat the Phase Q line properly:
- start from the architecture skeleton
- lock the boundary contracts in crate-local documents
- build the lint/parser gates before substantive implementation
- implement under those guardrails

This document is the execution tracker for that work.

## 2. Design Tasks

### R.0.1 Boundary Model Review

Status:
- complete

Required decisions:
- name the legal composition owner
- confirm `AtmProtocol` is the shared contract in `atm-core`
- confirm `ClientTransport` / `ServerTransport` split
- confirm thin-client workflow is `send` / `receive`
- confirm `ack` is folded into `send`-shape requests for thin clients
- confirm `NotificationSink` / `StatusSource` split
- add the missing watch/reconcile boundary

Acceptance:
- the boundary set in crate `boundaries.md` files is stable enough to review as
  design, not just schema

### R.0.2 Cross-Boundary Ownership Review

Status:
- complete

Required design clarifications:
- where cross-store orchestration lives
- where compatibility/recovery policy lives for:
  - `ConfigIngress`
  - `InboxIngress`
  - `InboxExport`
- where request dispatch ends and service orchestration begins
- whether remote daemon-to-daemon client behavior depends on the same
  `ClientTransport` boundary

Acceptance:
- each major boundary has a clear ownership line
- no major subsystem remains “named but still fuzzy”

### R.0.3 Composition Ownership Decision

Status:
- complete

Decision required:
- choose whether runtime wiring lives in:
  - `atm-daemon`
  - or a separate composition/app crate

Acceptance:
- the boundary inventories and crate architecture docs agree on the legal
  composition owner

## 3. Documentation Tasks

### R.1.1 Boundary Inventories

Status:
- complete

Artifacts:
- `docs/atm-core/boundaries.md`
- `docs/atm-rusqlite/boundaries.md`
- `docs/atm-daemon/boundaries.md`
- `docs/atm/boundaries.md`

Required follow-up:
- revise the records after R.0 design review
- add the missing watch/reconcile boundary

Acceptance:
- every major Phase R boundary is represented in one crate-local record

### R.1.6 Documentation Hardening Loop

Status:
- complete

Completed work:
- aligned top-level architecture and project-plan docs
- aligned crate architecture and requirements docs
- filled the missing watch/reconcile boundary family
- resolved the runtime composition-owner contradiction
- added crate-local ADR records for the key Phase R decisions
- ran repeated review/update loops over requirements, architecture, and
  boundary records until the remaining work moved out of documentation and
  into parser/lint or implementation execution

Acceptance:
- the documentation set is internally coherent enough to drive parser and lint
  work without relying on unstated architectural assumptions

### R.1.2 Top-Level Architecture Alignment

Status:
- complete

Required updates:
- make Phase R the active redesign line
- explicitly reference crate-local `boundaries.md` files
- describe:
  - `AtmProtocol`
  - `ClientTransport`
  - `ServerTransport`
  - `RequestDispatcher`
  - `NotificationSink`
  - `StatusSource`
- state that thin-client workflow is `send` / `receive`
- state that `ack` is folded into `send`-shape requests for thin clients

Acceptance:
- top-level architecture and crate boundary inventories describe the same system

### R.1.3 Crate Architecture Alignment

Status:
- complete

Required updates:
- align `docs/atm-core/architecture.md`
- align `docs/atm-daemon/architecture.md`
- align `docs/atm-rusqlite/architecture.md`
- align `docs/atm/architecture.md`

Acceptance:
- crate architecture docs and crate boundary inventories agree on:
  - ownership
  - composition
  - dependency direction
  - privacy rules

### R.1.4 Requirements Alignment

Status:
- complete

Required updates:
- state enforceable “must” rules for:
  - private concrete implementations
  - no CLI-to-daemon internal dependency
  - no CLI-to-SQLite dependency
  - shared protocol owned by `atm-core`
  - thin-client `send` / `receive` surface
  - watch/reconcile ownership once named

Acceptance:
- requirements contain only rules that can be checked or reviewed concretely

### R.1.5 ADR Records

Status:
- complete

Required ADRs:
- `AtmProtocol` ownership in `atm-core`
- `ack` folded into `send`
- split `ClientTransport` / `ServerTransport`
- concrete implementations remain private
- legal composition owner

Acceptance:
- each major Phase R design decision has one crate-local ADR record

## 4. Downstream Execution Phases

The items below are not part of the completed documentation hardening loop.
They depend on the hardened document set above and move into parser, lint, and
implementation execution.

### R.2 Tooling

### R.2.1 Boundary Parser

Status:
- in progress by `arch-inj`

Scope:
- parse crate-local boundary records
- validate basic record structure

Acceptance:
- parser can read all current `boundaries.md` files without ambiguity

### R.2.2 Lint Gates

Status:
- pending

Initial lint passes:
- schema validation
- manifest dependency-edge checks
- forbidden external-reference checks

Deferred until after design freeze:
- composition-root enforcement
- deeper visibility/re-export checks

Acceptance:
- `just lint` can fail on the first hard architectural violations

### R.3 Implementation

### R.3.1 Skeleton First

Status:
- pending

Required outcome:
- traits/facades exist
- private implementation shells exist
- composition point exists
- illegal references are already blocked by lint and visibility

Acceptance:
- the architecture can compile in skeleton form before feature behavior lands

### R.3.2 Behavior Sprints

Status:
- pending

Required order:
1. protocol and transport skeleton
2. store boundary skeleton
3. config/inbox/notifier/watch boundaries
4. service orchestration
5. thin client surfaces

Acceptance:
- no feature sprint begins before the relevant boundary and lint guardrails are in place

## 6. Working Rule

Phase R does not advance by ad hoc implementation.

Required order:
1. design decision
2. boundary record
3. architecture/requirements/ADR alignment
4. lint/parser support
5. implementation skeleton
6. feature behavior
