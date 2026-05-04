# Phase R Task List

## 1. Goal

Repeat the Phase Q line properly:
- start from the architecture skeleton
- lock the boundary contracts in crate-local documents
- build the lint/parser gates before substantive implementation
- implement under those guardrails

This document is the execution tracker for that work.

## 2. Current Planning Baseline

Status:
- design and documentation baseline complete

Completed baseline:
- boundary model review completed
- cross-boundary ownership review completed
- composition ownership documented
- crate-local boundary inventories written
- top-level architecture, requirements, project plan, and crate docs aligned
- documentation hardening loop completed

Current completeness:
- documentation/design contract: strong first complete draft
- lint/parser execution: started
- implementation skeleton: not started
- behavior sprints: not replanned yet against the new skeleton

Current branch-state checks:
- boundary parser/lint foundation is in progress on `feature/pR-s0-arch-foundation`
- current `just lint` on that branch fails in two active areas:
  - shear:
    - `tests/support/mod.rs` `unlinked_files`
    - `src/config/bridge.rs` `empty_files`
    - `src/log/filters.rs` `empty_files`
    - `src/mailbox/hash.rs` `empty_files`
  - identities:
    - `RULE-008` / `RULE-009`
    - about `880` current findings

## 3. Completed Design Work

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

## 4. Completed Documentation Work

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

## 5. Active Execution Phases

The items below are the active Phase R execution order. They depend on the
hardened document set above and should drive the next implementation steps.

### Wave 1: Boundary Establishment And Enforcement

Wave 1 uses lint, parser, and skeleton work to establish and enforce hard code
boundaries before substantive implementation begins.

Primary Wave 1 deliverable:
- the new Phase R skeleton:
  - new crates
  - public boundary traits/facades
  - major data structures

Supporting prerequisites inside Wave 1:
- `R.0` lint foundation
- `R.1` current lint debt burn-down
- `R.2A` parallel lint hardening

### R.0 Lint Foundation

### R.0.1 Boundary Parser

Status:
- in progress by `arch-inj`

Scope:
- parse crate-local boundary records
- validate basic record structure

Acceptance:
- parser can read all current `boundaries.md` files without ambiguity

### R.0.2 Boundary Lint Gates

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

Acceptance:
- `just lint` can fail on the first hard architectural violations

### R.1 Current Lint Debt Burn-Down

Status:
- pending

Required outcome:
- make the current lint baseline actionable before skeleton work expands
- either fix or explicitly reclassify any non-architectural backlog that blocks
  the new boundary checks from becoming useful

Current failure set:
- shear:
  - `tests/support/mod.rs` `unlinked_files`
  - `src/config/bridge.rs` `empty_files`
  - `src/log/filters.rs` `empty_files`
  - `src/mailbox/hash.rs` `empty_files`
- identities:
  - `RULE-008` / `RULE-009`
  - about `880` current findings on the current lint branch baseline

Acceptance:
- `just lint` either passes on the current baseline or fails only on the
  intentional next architectural gaps being addressed by the active sprint

### R.2 Skeleton Crates And Boundary Traits

Status:
- pending

Required outcome:
- create the new Phase R crate/module skeleton needed by the hardened boundary
  design
- define the public boundary traits/facades first
- define the major data structures needed by the new boundary-owned surfaces
- create private implementation shells where the design already identifies the
  concrete owner
- give lint concrete ownership surfaces to validate against

Required shape:
- new Phase R crate/module layout
- `AtmProtocol` contract in `atm-core`
- `ClientTransport` / `ServerTransport` traits
- `RequestDispatcher` trait/facade
- store boundary traits
- major protocol/store/config/inbox boundary-owned data structures
- config/inbox/notification/status/watch/reconcile traits
- composition roots in `atm` and `atm-daemon`

Acceptance:
- the architecture can compile in skeleton form
- lint has real crates/modules/traits/impl shells to inspect
- Wave 1 produces the concrete boundary-owned skeleton that later implementation
  sprints build on

### R.2A Parallel Lint Hardening

Status:
- pending

Required outcome:
- run a second lint-improvement round in parallel with the skeleton sprint
- use the newly created crate/module/trait surfaces from `R.2` to harden the
  architectural checks beyond the current parser-first baseline
- tighten the tooling before behavior work begins

Focus areas:
- composition-root enforcement
- stronger privacy / constructor / re-export enforcement
- crate-local forbidden-import precision
- improved boundary-to-code path mapping
- better diagnostics for ownership and dependency violations

Acceptance:
- the lint suite can validate the new skeleton at a more concrete level than
  the current document-and-manifest-only baseline

### R.3 Sprint Replan After Skeleton

Status:
- pending

Required outcome:
- once the skeleton and lint baseline exist, rewrite the remaining implementation
  work into concrete Phase R sprints
- plan the actual work against the new crates, public traits, and legal
  composition roots rather than against Phase Q carry-over modules
- incorporate the results of both:
  - `R.2` skeleton creation
  - `R.2A` lint hardening

Acceptance:
- the remaining Phase R work is expressed as concrete implementation sprints on
  top of the enforced skeleton

### Wave 2: Implementation Against Enforced Boundaries

Wave 2 executes implementation work only after Wave 1 has established the lint
gates, cleaned the baseline enough to make them useful, and created the new
crate/trait skeleton that those gates can check concretely.

### R.4 Implementation

### R.4.1 Skeleton First

Status:
- pending

Required outcome:
- traits/facades exist
- private implementation shells exist
- composition point exists
- illegal references are already blocked by lint and visibility

Acceptance:
- the architecture can compile in skeleton form before feature behavior lands

### R.4.2 Behavior Sprints

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
5. current lint debt burn-down
6. implementation skeleton
7. parallel lint hardening on the skeleton
8. sprint replanning
9. feature behavior
