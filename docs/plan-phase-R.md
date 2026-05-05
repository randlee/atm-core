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
- composition-root enforcement (carry into `R.4`)
- cargo-modules cycle gating beyond false-positive review (carry into `R.4`)
- unsafe view hardening beyond cargo-geiger package-resolution failures (carry into `R.6`)

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
- config ingestion and inbox ingress/export adapter shells
- final module splits that move daemon/runtime and sqlite adapters out of the
  current crate-root skeleton files
- any future composition path that connects runtime wiring to sqlite-backed
  adapters without introducing a direct `atm-daemon -> atm-rusqlite` edge
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
- complete

Current landed subset:
- `crates/atm-daemon` and `crates/atm-rusqlite` exist as crate-root skeletons
- `crates/atm-core/src/boundary/mod.rs` now carries stub traits plus request/result shells for:
  - `MailStore`
  - `TaskStore`
  - `RosterStore`
  - `ConfigIngress`
  - `InboxIngress`
  - `InboxExport`
- daemon runtime stub adapters are landed for:
  - `ServerTransport`
  - `NotificationSink`
  - `StatusSource`
  - `WatchEventSource`
  - `ReconcileCoordinator`
- sqlite stub adapters are landed for:
  - `MailStore`
  - `TaskStore`
  - `RosterStore`
- explicit composition modules exist in:
  - `crates/atm-daemon/src/composition.rs`
  - `crates/atm/src/composition.rs`
- `just lint` passes on the current skeleton branch with:
  - no direct CLI-to-daemon edge
  - no direct CLI-to-sqlite edge
  - no direct daemon-to-sqlite edge permitted by the boundary contract
- daemon boundary inventory now records landed stub runtime adapters for:
  - `ConfigIngress`
  - `InboxIngress`
  - `InboxExport`
- `PeerClientTransport` and `RequestDispatcher` daemon runtime adapters are
  formally deferred to `R.4` scope review rather than blocking skeleton close

Follow-on work after `R.3.1` close:
- final daemon/rusqlite adapter module splits beyond the current crate-root skeletons
- a trait-only composition path for sqlite-backed runtime assembly that does not
  require a direct `atm-daemon -> atm-rusqlite` dependency
- `R.4` scope review for:
  - peer client transport
  - request dispatcher

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

### R.3.2 Behavior Sprint Review

Status:
- in progress

Purpose:
- review, drill, and finalize the proposed `R.4` through `R.8` sprint scopes
- identify open scope decisions before Wave 2 implementation begins
- convert the current wave outline into reviewable sprint checklists, not to
  declare those sprints implicitly approved

Required order:
1. protocol and transport
2. store boundaries
3. config / inbox / notifier / watch boundaries
4. service orchestration
5. thin client surfaces

Review targets:
1. `R.4 Protocol + Transport`
   - Scope review required first:
     - `ServerTransport`, `NotificationSink`, `StatusSource`,
       `WatchEventSource`, and `ReconcileCoordinator` currently have zero
       methods; define their minimal callable method surfaces before behavior
       work starts
     - no `R.4` implementation may start until that scope review is documented
       and approved by team-lead
     - `PeerClientTransport` and `DaemonRequestDispatcher`: define trait
       surfaces in `R.4`; concrete daemon adapter implementations are deferred
       to a later sprint
   - `crates/atm-core/src/boundary/mod.rs`
     - replace placeholder `AtmRequestEnvelope`, `AtmResponseEnvelope`, and
       `AtmFramePayload` with:
       - `RequestEnvelope`
       - `ResponseEnvelope`
       - `FramePayload`
       in an explicit `atm_core::protocol` module or equivalent architecturally
       correct home
     - add callable methods to:
       - `AtmProtocol`
       - `ClientTransport`
       - `ServerTransport`
       - `RequestDispatcher`
       - `NotificationSink`
       - `StatusSource`
       - `WatchEventSource`
       - `ReconcileCoordinator`
   - `crates/atm-daemon/src/lib.rs`
     - implement stub method signatures for runtime-owned adapters:
       - `LocalSocketServerTransport`
       - `DaemonNotificationSink`
       - `DaemonStatusSource`
       - `FileWatchEventSource`
       - `DaemonReconcileCoordinator`
     - when any new concrete runtime impl structs land, update the matching
       boundary records with explicit `implementation.visibility` and
       `implementation.constructor` expectations in the same sprint
   - `crates/atm-daemon/src/composition.rs`
     - wire runtime composition to the new transport/dispatcher method surfaces
       without introducing direct CLI or sqlite dependencies
     - carry forward the remaining `R.3.1` runtime-composition residuals:
       - define the trait-only composition path that preserves no direct
         `atm-daemon -> atm-rusqlite` dependency
       - map any remaining daemon/runtime module-split work needed by transport
         and dispatcher ownership
   - `crates/atm/src/composition.rs`
     - wire `CliComposition` against the `ClientTransport` method surface only
   - Acceptance:
     - `RequestEnvelope`, `ResponseEnvelope`, and `FramePayload` are named
       protocol DTO targets and exported from the agreed protocol home
     - zero-method runtime traits resolved by explicit method surfaces
     - CLI and daemon compositions compile against callable transport traits
     - no direct `atm -> atm-daemon` or `atm -> atm-rusqlite` edge appears
     - verify `lint_boundaries.py` rejects any impl of boundary traits outside
       permitted impl sites documented in `docs/*/boundaries.md`
     - verify the remaining open ADR-001 action item is closed by confirming
       `lint_boundaries.py` and the boundary records reflect all current
       permitted impl sites
     - verify the `#[doc(hidden)]` ADR-001 action item is closed in the landed
       `atm-core` boundary module implementation
     - any new concrete implementation struct introduced in this sprint must:
       (a) add boundary-record visibility/constructor rules; (b) have boundary
       lint enforce them; (c) pass QA verification of those checks
     - QA verifies any new runtime impl structs are covered by active privacy /
       constructor lint checks
2. `R.5 Store Boundaries`
   - Start gate:
     - `R.5` may not begin until `R.4` acceptance criteria are signed off by
       team-lead
   - `crates/atm-core/src/boundary/mod.rs`
     - finalize request / response DTOs for:
       - `MailStore`
       - `TaskStore`
       - `RosterStore`
     - ensure method families match actual retained behaviors:
       - message persistence / visibility / replay state
       - task creation / update / ack transition / message links
       - roster replace / load / membership query / health
   - `crates/atm-rusqlite/src/lib.rs`
     - replace typed stub failures with real trait implementations for:
       - `SqliteMailStore`
       - `SqliteTaskStore`
       - `SqliteRosterStore`
     - keep constructors private and assembly boundary-facing only
     - keep boundary records and lint privacy rules in lockstep with every new
       concrete store implementation struct
     - carry forward the remaining `R.3.1` sqlite residuals:
       - complete the adapter/module split beyond the current crate-root
         skeleton file
       - keep the runtime-to-sqlite path trait-only rather than a direct daemon
         dependency
   - Retained behavior cutover:
     - identify and replace direct store ownership in existing retained flows
       under:
       - `crates/atm-core/src/read/`
       - `crates/atm-core/src/clear/`
       - `crates/atm-core/src/send/`
       - `crates/atm-core/src/ack/`
       - `crates/atm-core/src/team_admin/`
   - Tests:
     - add store-contract coverage in `crates/atm-core/tests/`
     - keep adapter-specific behavior tests in `crates/atm-rusqlite`
   - Acceptance:
     - SQLite-backed behavior lives behind `MailStore` / `TaskStore` /
       `RosterStore`
     - retained core flows no longer own sqlite-facing logic directly
     - replacing the sqlite adapter does not require caller changes outside
       composition or adapter crates
     - any new concrete implementation struct introduced in this sprint must:
       (a) add boundary-record visibility/constructor rules; (b) have boundary
       lint enforce them; (c) pass QA verification of those checks
     - QA verifies store impl structs remain private and lint-enforced as such
3. `R.6 Config / Inbox / Notification / Watch`
   - `crates/atm-core/src/boundary/mod.rs`
     - finalize method surfaces and DTOs for:
       - `ConfigIngress`
       - `InboxIngress`
       - `InboxExport`
       - `NotificationSink`
       - `StatusSource`
       - `WatchEventSource`
       - `ReconcileCoordinator`
   - `crates/atm-daemon/src/lib.rs`
     - implement real daemon-owned adapters for:
       - config loading
       - inbox import/export
       - notification delivery
       - status reporting
       - watch capture
       - reconcile coordination
     - keep boundary records and lint privacy expectations updated for every
       newly landed daemon-owned implementation struct
   - Policy placement review:
     - document and implement where compatibility / recovery policy is allowed
       to live inside ingress/export adapters versus service orchestration
   - Retained behavior cutover:
     - remove direct config parsing, inbox compatibility handling, and watch
       ownership from retained command/service code
     - carry forward the remaining `R.3.1` service-shell residuals for these
       domains before R.7 final orchestration cutover
   - Acceptance:
     - config/inbox/notification/watch behavior is owned by explicit adapters
     - retained service code consumes those behaviors only through boundary
       traits
     - compatibility policy location is documented and matches implementation
     - any new concrete implementation struct introduced in this sprint must:
       (a) add boundary-record visibility/constructor rules; (b) have boundary
       lint enforce them; (c) pass QA verification of those checks
     - QA verifies newly introduced adapter impl structs are covered by privacy
       and constructor lint rules
4. `R.7 Service Orchestration`
   - Files in scope:
     - `crates/atm-core/src/send/`
     - `crates/atm-core/src/read/`
     - `crates/atm-core/src/clear/`
     - `crates/atm-core/src/ack/`
     - `crates/atm-core/src/doctor/`
     - retained shared helpers those flows still call directly
   - Required routing changes:
     - all retained command/service flows call boundary traits or service-owned
       orchestration seams only
     - remove parallel helper paths that bypass:
       - store boundaries
       - config ingress
       - inbox ingress/export
       - notification / status / watch adapters
   - Composition constraints:
     - daemon composition and CLI composition remain the only legal wiring roots
     - no direct adapter construction from retained command modules
   - Acceptance:
     - direct retained bypasses are removed from service code
     - orchestration layer is explicit and thin
     - boundary lint remains green after routing changes
     - any new concrete implementation struct introduced in this sprint must:
       (a) add boundary-record visibility/constructor rules; (b) have boundary
       lint enforce them; (c) pass QA verification of those checks
     - QA verifies no orchestration change required widening adapter visibility
       or bypassing boundary privacy rules
5. `R.8 Thin Client Surfaces`
   - `crates/atm/src/`
     - finalize CLI composition around:
       - `ClientTransport`
       - observability port
       - thin `send` entry point
       - thin `receive` entry point
     - remove or isolate any retained command construction path that bypasses
       the composition module
   - Shared protocol surface:
     - keep `ack` folded into send-shaped requests rather than a separate
       top-level thin-client method family
   - Extension readiness:
     - ensure `atm-graft`-style thin client callers can stop at
       `AtmProtocol` + `ClientTransport` without daemon or sqlite references
   - Acceptance:
     - CLI public surface is thin and transport-driven
     - `ack` remains modeled inside `send`
     - thin clients do not require daemon-internal or sqlite-facing knowledge
     - daemon lifecycle (`start` / `stop` / `health`) and all currently
       supported `atm` CLI commands remain functional at `R.8` close
     - auto-start when the daemon is absent remains supported, with no silent
       fallback to in-process execution
     - `lint_manifests.py` confirms the following ADR-001 dependency edges
       remain FORBIDDEN:
       - `atm -> atm-daemon`
       - `atm -> atm-rusqlite`
       - `atm-core -> atm-daemon`
       - `atm-core -> atm-rusqlite`
       - `atm-daemon -> atm-rusqlite` (trait-only/reference-only)
     - any new concrete implementation struct introduced in this sprint must:
       (a) add boundary-record visibility/constructor rules; (b) have boundary
       lint enforce them; (c) pass QA verification of those checks
     - QA verifies no thin-client change introduces direct references to daemon
       or adapter implementation structs

Acceptance:
- no feature sprint begins before the relevant boundary and lint guardrails are in place
- `R.4` through `R.8` are reviewable as concrete sprint proposals with explicit
  files, traits, and acceptance criteria

Cross-sprint hardening rule:
- whenever a sprint introduces a new concrete implementation struct for a
  boundary, that same sprint must also:
  - add or update the `boundaries.md` record for that implementation
  - set explicit `implementation.visibility` and
    `implementation.constructor` requirements
  - ensure boundary lint actively enforces those privacy expectations
  - include QA verification that the privacy / constructor / re-export rules
    are present and passing in `just lint`
- ADR-001 AGENTS.md guard note:
  - complete at `cd70665`; no further sprint ownership needed unless the
    prompt location changes again

## 6. Working Rule

Phase R does not advance by ad hoc implementation.

Required order:
1. design decision
2. boundary record
3. architecture/requirements/ADR alignment
4. lint/parser support
5. implementation skeleton
6. feature behavior
