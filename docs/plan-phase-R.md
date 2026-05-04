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
- in progress

Initial lint passes:
- schema validation
- manifest dependency-edge checks
- forbidden external-reference checks
- active impl privacy / constructor / re-export checks
- owner-crate test-bypass checks

Deferred until after design freeze:
- composition-root enforcement
- cargo-modules cycle gating beyond false-positive review
- unsafe view hardening beyond cargo-geiger package-resolution failures

Acceptance:
- `just lint` can fail on the first hard architectural violations

### R.3 Implementation

### R.3.0 Baseline Review

Status:
- complete

Merged `integrate/phase-R` baseline reviewed at:
- `dbe1eef` from the sprint brief
- current worktree baseline after sync

What is already landed:
- `crates/atm-core/src/boundary/mod.rs` contains the first protocol/runtime
  trait stubs and placeholder data structures for:
  - `AtmProtocol`
  - `ClientTransport`
  - `ServerTransport`
  - `RequestDispatcher`
  - `NotificationSink`
  - `StatusSource`
  - `WatchEventSource`
  - `ReconcileCoordinator`
- boundary docs, ADR alignment, and the initial boundary-enforcement lint suite
  are in place
- `just lint` is already useful for:
  - boundary schema and duplicate checks
  - owner package / manifest consistency
  - allowed-dependent / forbidden-edge checks
  - forbidden external reference checks
  - active implementation privacy / constructor / re-export checks
  - owner-crate test-bypass checks

What is not landed yet:
- `crates/atm-daemon/` crate scaffold
- `crates/atm-rusqlite/` crate scaffold
- `MailStore`, `TaskStore`, `RosterStore`, `ConfigIngress`, `InboxIngress`, and
  `InboxExport` Rust traits
- adapter implementation shells for daemon/runtime, SQLite, config ingestion,
  inbox ingestion/export, and notification/status plumbing
- explicit composition modules that wire the CLI client root and daemon runtime
  root through only the boundary contracts
- service orchestration shells that route retained command behavior through the
  new boundary-owned call graph

Gate status before Wave 2:
- authoritative now:
  - boundary/manifests/reference/privacy lint checks
- still tooling work or view-only:
  - composition-root enforcement
  - `cargo-modules --acyclic` cycle gating
  - Graphviz-backed module view generation
  - cargo-geiger-backed unsafe view generation

### R.3.1 Skeleton First

Status:
- pending

Required outcome:
- traits/facades exist
- private implementation shells exist
- composition point exists
- illegal references are already blocked by lint and visibility

Concrete checklist:
1. `crates/atm-daemon`
   - scaffold crate, manifest, and `src/lib.rs`
   - add private runtime adapter shells for:
     - `ServerTransport`
     - `NotificationSink`
     - `StatusSource`
     - `WatchEventSource`
     - `ReconcileCoordinator`
   - add daemon composition module that becomes the only runtime wiring root
2. `crates/atm-rusqlite`
   - scaffold crate, manifest, and `src/lib.rs`
   - add private adapter shells for:
     - `MailStore`
     - `TaskStore`
     - `RosterStore`
   - keep constructors private and expose only boundary-facing assembly hooks
3. `crates/atm-core`
   - extend `src/boundary/` beyond protocol/runtime stubs
   - land Rust trait definitions plus request/result/error shells for:
     - `MailStore`
     - `TaskStore`
     - `RosterStore`
     - `ConfigIngress`
     - `InboxIngress`
     - `InboxExport`
   - tighten protocol placeholder structures into named request/response/frame
     types that the client/server transports and dispatcher will share
4. `crates/atm`
   - add an explicit client composition module that wires only:
     - `ClientTransport`
     - observability
     - thin `send` / `receive` command entry points
   - keep retained CLI behavior compiling while routing new construction through
     the Phase R composition surface
5. Shared data structures
   - create the major boundary-owned DTO shells required by the first behavior
     sprints:
     - protocol request/response envelopes
     - store query/command result shapes
     - config/inbox import-export request and result shells
     - notification/status/watch/reconcile event shells
6. Lint compatibility required before closing `R.3.1`
   - boundary records must point at landed crate/module paths
   - no new public concrete adapter constructors
   - no illegal caller edges to daemon internals or SQLite crates

Acceptance:
- the architecture can compile in skeleton form before feature behavior lands

### R.3.2 Behavior Sprints

Status:
- pending

Required order:
1. protocol and transport
2. store boundaries
3. config / inbox / notifier / watch boundaries
4. service orchestration
5. thin client surfaces

Ordered sprint breakdown:
1. `R.4 Protocol + Transport`
   - harden `AtmProtocol` request/response/frame types
   - land callable `ClientTransport` and `ServerTransport` trait surfaces
   - land `RequestDispatcher` request-routing contract
   - connect CLI composition root and daemon composition root to these
     contracts without introducing retained direct call paths
2. `R.5 Store Boundaries`
   - implement `MailStore`, `TaskStore`, and `RosterStore` trait contracts in
     `atm-core`
   - land private SQLite adapter shells in `atm-rusqlite`
   - move current retained direct SQLite ownership behind those contracts
3. `R.6 Config / Inbox / Notification / Watch`
   - land `ConfigIngress`, `InboxIngress`, and `InboxExport`
   - land `NotificationSink`, `StatusSource`, `WatchEventSource`, and
     `ReconcileCoordinator`
   - decide and implement the retained compatibility-policy locations inside
     those adapters
4. `R.7 Service Orchestration`
   - route retained core command flows through the boundary-owned contracts
   - eliminate direct retained call paths that bypass stores, config, inbox, or
     runtime adapters
   - make daemon runtime and CLI composition roots the only legal wiring points
5. `R.8 Thin Client Surfaces`
   - reshape CLI/graft-facing surfaces around thin `send` / `receive`
   - keep `ack` folded into send-shaped requests
   - finalize the public client surface once the service graph is stable

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
