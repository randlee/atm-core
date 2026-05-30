# Phase R Task List

## 1. Goal

Repeat the abandoned early SQLite/daemon line properly, under enforced boundaries:
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
- ADR-004: structured boundary definitions (lives on
  `feature/pR-s3-boundary-lint`)

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
- `just lint sc-boundary` passes on the current skeleton branch with:
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
     - formal R.6 disposition:
       daemon-owned config/inbox/watch adapters land in this sprint, but the
       retained `send` / `read` / `ack` / `clear` command-family cutover to
       `ConfigIngress` / `InboxIngress` / `InboxExport` remains deferred to
       `R.7`
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
     - REQ-P-RUNTIME-001 preserved at `R.8` close:
       - daemon auto-start when absent remains supported
       - auto-start failure emits a typed actionable error and recovery
         guidance
       - no production path may silently fall back to direct SQLite or
         inbox-file access
     - daemon lifecycle (`start` / `stop` / `health`) and all currently
       supported `atm` CLI commands remain functional at `R.8` close
     - `lint_boundaries.py` confirms the following ADR-001 dependency edges
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

## 5. Phase R Branch Consolidation (2026-05-05)

### Policy

All Phase R stabilization work routes exclusively to `feature/pR-s10-thin-client`. Branches `feature/pR-s8-config-notify` (R.6) and `feature/pR-s9-service-orch` (R.7) are frozen and treated as historical record only.

**Surviving branch:** `feature/pR-s10-thin-client` (PR #181)
**Frozen branches:** `feature/pR-s8-config-notify` (PR #182), `feature/pR-s9-service-orch` (PR #180)

### Rationale

RULE-011 and ARCH-SINGLETON requirements were added to develop after the per-sprint QA passes ran. Applying retroactive fixes branch-by-branch would require three separate remediation rounds on overlapping codebases. Consolidating to R.8 eliminates duplicate work and provides one clear merge path to `integrate/phase-R`.

### Ancestry

Verified 2026-05-05: merge-forward is complete.
- 75b5031 (R.6 head) is ancestor of cc3a70a (R.7 head)
- cc3a70a (R.7 head) is ancestor of fc604ce (R.8 head)
- No further merge-forward needed.

### Enforcement

- No QA rounds on frozen branches. Findings on R.6 or R.7 are superseded.
- No commits to `feature/pR-s8-config-notify` or `feature/pR-s9-service-orch`.
- All daemon fixes, ARCH-SINGLETON sweep, CI-WIN-001, and carry-forward findings apply to `feature/pR-s10-thin-client` only.
- quality-mgr: reject any new assignments targeting frozen branches.

### Open Findings on R.8 (TASK-939 scope)

**Blocking:**
- ARCH-SINGLETON [B]: `spawn_test_daemon`/`DaemonGuard` in `crates/atm/tests/send.rs` and other test files — replace with `CliComposition::from_transport()` + in-process `FakeClientTransport`
- CI-WIN-001 [B]: ungated unix-only imports in `atm-daemon/src/lib.rs` — gate with `#[cfg(unix)]`

**Important carry-forward (R.6/R.7/R.8):**
- ATM-QA-014, ATM-QA-006, ATM-QA-009, FTQ-006, NEW-004, NEW-002

**Minor carry-forward:**
- ATM-QA-005

## 5.1 R.9 Planning Task List

Status:
- in progress on `feature/pR-s9-singleton-planning`

Scope:
- convert daemon singleton and test fidelity from scattered review comments
  into explicit requirements, ADRs, testing guidance, and implementation
  planning

Task list:
1. strengthen product and crate requirements so singleton is daemon
   requirement `#1`
2. explicitly prohibit the current daemon-spawn test pattern by name:
   - `spawn_test_daemon`
   - `warm_daemon`
   - `DaemonGuard`
   - `ATM_DAEMON_BIN`
   - direct `Command::new(...atm-daemon...)`
3. define at least two runtime singleton guard layers plus one lint/CI gate
4. write ADR-002 for host-wide daemon singleton
5. write ADR-003 for test fidelity and daemon isolation
6. define the singleton lint gate and decide whether existing tools are
   sufficient
   - decision: existing generic tools are not sufficient by themselves;
     add `scripts/lint_daemon_singleton.py` as a dedicated repository lint
     integrated into `just lint`
7. define the approved test tiers:
   - `FakeClientTransport`
   - loopback/in-process transport
   - narrow daemon-runtime harness
8. map the planning response to current findings:
   - ARCH-SINGLETON
   - CI-WIN-001
   - singleton review findings `RBP-F001` through `RBP-F012`
   - ATM-QA-014
   - ATM-QA-006
   - ATM-QA-009
   - NEW-004
   - NEW-002
   - ATM-QA-005
   - FTQ-006

Acceptance:
- the requirements/ADR/plan set is explicit enough to guide implementation
  without re-litigating the singleton rule

## 5.2 R.10 Implementation Task List

Status:
- planned

Execution slices:

### R.10.1 Runtime Singleton Hardening

- add the client-side pre-spawn launch gate before daemon fork/exec
- keep the daemon-side startup gate as the final ownership rejection layer
- harden stale-owner recovery without allowing split ownership
- make startup failure typed and deterministic when ownership is already held
- ensure signal installation and stale socket cleanup are idempotent and
  correctly surfaced
- gate Unix-only daemon runtime code explicitly so Windows CI does not compile
  unsupported imports or paths by accident

Directly addresses:
- RBP-F003
- RBP-F011
- RBP-F012
- ARCH-SINGLETON
- CI-WIN-001

### R.10.2 Boundary And API Hardening

- make `ClientTransport` include `Send + Sync`
- separate daemon supervision from transport construction
- add semantic path newtypes for daemon binary and socket path
  - `DaemonBinaryPath` and `DaemonSocketPath`
  - invariants: non-empty and valid UTF-8 path representation at the boundary
  - both types must implement `AsRef<Path>` for ergonomic filesystem call-site
    use
  - failures return typed parse/validation errors rather than panic/expect
- remove unreachable stub patterns that hide impossible paths behind routine
  `Result`
- resolve deadline-overrun semantics so callers can distinguish committed work
  from clean rejection
- bound daemon request framing instead of unbounded `read_to_end`
- inject daemon home/observability dependencies once rather than recomputing
  them per request
- fix fixture/setup boundary ambiguities and missing command-local environment
  injection coverage identified by NEW-004 and NEW-002

Directly addresses:
- RBP-F004
- RBP-F005
- RBP-F007
- RBP-F008
- RBP-F009
- ATM-QA-014
- ATM-QA-006
- NEW-004
- NEW-002

### R.10.3 Test Fidelity Migration

- delete `spawn_test_daemon` and `DaemonGuard`
- remove `warm_daemon` from ordinary CLI tests
- delete `ATM_DAEMON_BIN`-driven daemon launch from ordinary tests
- replace routine CLI daemon usage with `CliComposition::from_transport(...)`
  plus `FakeClientTransport`
- add loopback/in-process transport where request/handler integration needs
  more realism than a pure fake
- fix `EnvGuard` ownership so test environment cleanup cannot race
- gate Unix-only daemon-dependent tests explicitly or migrate them to approved
  in-process seams
- remove obsolete launcher-only panic paths and unused helper parameters as the
  daemon-spawn helpers disappear
- resolve remaining test-harness shutdown semantics such as ATM-QA-005 inside
  the Tier 3 daemon-runtime suite rather than leaving them as implicit polling
  behavior

Directly addresses:
- RBP-F001
- RBP-F002
- RBP-F006
- RBP-F010
- ATM-QA-009
- ATM-QA-005
- FTQ-006

### R.10.4 Lint Gate Delivery

- add a dedicated repository lint to `just lint`
- script entrypoint: `scripts/lint_daemon_singleton.py`
- scan test code for prohibited daemon-spawn patterns:
  - `spawn_test_daemon`
  - `warm_daemon`
  - `DaemonGuard`
  - `ATM_DAEMON_BIN`
  - `atm-daemon.sock`
  - direct `Command::new(...atm-daemon...)`
  - timing-based daemon warmup shortcuts in ordinary tests
- document the allowed exceptions for the narrow daemon-runtime suite
- include platform gating checks for Unix-only daemon-runtime code where the
  default workspace targets Windows CI too

Acceptance:
- no new daemon-spawn pattern can land without a deliberate lint/CI change
- lint gate must document explicit allow-list for Tier 3 daemon-runtime suite
  patterns; allowed exceptions for the narrow daemon-runtime suite are governed
  by `docs/testing-guidelines.md §4.3`

### Merge Sequence (pending QA PASS 0B+0I+0m + user authorization)

1. `feature/pR-s10-thin-client` → `integrate/phase-R` (after R.5 #179 already merged)
2. `integrate/phase-R` → `develop` (user authorization required)

## 5.3 Phase R Continuation Sprints

Status:
- planned

Notes:
- `R.11` is already referenced by accepted limitations and remains reserved
- `R.12` is already taken in the team sprint ledger
- the next new implementation sprint identifier is `R.13`
- `sc-lint` inventory-parity / planning-metadata support is treated as an
  external prerequisite that must be ready before `R.13` begins

Execution sequence:

### R.13 Runtime Admission And Lifecycle

Status:
- implemented on `feature/pR-s13-runtime-admission`

- close B-001, B-002, and B-003
- move both daemon lock paths to one host-wide ownership root
- implement a real `RuntimeComposition::start()` lifecycle path
- absorb lifecycle-adjacent shutdown hardening from I-001 and I-002
- delivered host runtime ownership under `~/.atm/daemon/{launch.lock,owner.lock}`
- route `run_daemon()` only through `RuntimeComposition::start()`
- add typed lifecycle states and rollback on failed startup
- reject direct `LocalSocketServerTransport::serve()` bootstrap outside
  `RuntimeComposition::start()`
- preserve pending terminate/reload bits across repeated signal installs
- perform bounded stale-owner recovery retries before emitting
  `ATM_DAEMON_STALE_OWNER_RECOVERY_FAILED`
- route listener/accept failures through the same `Running -> Draining ->
  Stopped` shutdown path as signal-driven termination
- sprint plan: `docs/phase-R/sprint-R13.md`

### R.14 SQLite Root And Message-Thread Semantics

- close the host-scoped SQLite root change under `~/.atm/db/mail.db`
- implement the linear successor-chain model (`add-details` / `supersede`)
- keep `atm ack` as one visible reply with `requires_ack = false`
- implement ephemeral stale-time retention without read-triggered deletion
- finish SQLite error-mapping and test-fixture policy updates
- sprint plan: `docs/phase-R/sprint-R14.md`

### R.15 Heartbeat, Status Cache, And Doctor Health

- close B-004, B-005, and B-006
- implement runtime-owned heartbeat/member state
- implement daemon-owned live status cache
- wire doctor to daemon-backed liveness/readiness projection
- sprint plan: `docs/phase-R/sprint-R15.md`

### R.16 Peer Delivery And Replay

- close B-009 and I-013
- replace `PeerClientTransport` stub with real outbound daemon-to-daemon
  transport
- wire durable replay/re-export around the outbound peer path
- sprint plan: `docs/phase-R/sprint-R16.md`

### R.17 Watch, Reconcile, And Notifier Runtime

- close B-007, B-008, and I-012
- replace one-shot boundary-support helpers with runtime-owned watch and
  reconcile loops
- add daemon-owned notifier/plugin runtime delivery
- sprint plan: `docs/phase-R/sprint-R17.md`

### R.18 Production Hardening And Closeout

- close I-003 through I-016 that remain after the runtime-lane sprints
- finish config reload, request-id, type-safety, env/test portability, and
  remaining runtime-boundary hardening
- update plan/requirements/architecture/boundaries to the final landed state
- sprint plan: `docs/phase-R/sprint-R18.md`

### R.19 Postmortem Linter Backfill

Status:
- completed on `feature/pR-postmortem-linters`

Goal:
- convert the recurring mechanically-detectable Phase R finding families into
  normal repository lint or CI gates
- prove those rules on `atm-core` first, then migrate the reusable subset into
  standalone `sc-lint`

Partition:
- reusable rules that should begin on `atm-core` and later migrate into
  `sc-lint`:
  - Unix platform-gating enforcement
  - bare production `Condvar::wait(...)` enforcement
- ATM-local rules that should begin and remain on `atm-core` unless they later
  stabilize into generic frameworks:
  - duplicate semantic string-literal enforcement in non-test Rust code
  - fixed-sleep test-hygiene enforcement
  - triage Turtle consistency enforcement

Execution sequence:
1. extend `sc-portability` for:
   - ungated `std::os::unix` imports
   - `cfg_attr(not(unix), allow(dead_code))` portability suppressors
2. extend the existing identity-literal lint into a duplicate semantic
   string-literal gate for non-test Rust code, with raw `”team-lead”` as the
   first mandatory case
3. add a new repository-local fixed-sleep test-hygiene lint and wire it into
   `just lint`
4. extend `sc-boundary` for bare production `Condvar::wait(...)`
   - replacement code must inspect the returned `WaitTimeoutResult`; swapping
     to `wait_timeout(...)` while discarding timeout state is still a bug
5. add a repository-local triage-record consistency lint/CI check and wire it
   into the default developer gate
6. after all families are green and low-noise on `atm-core`, extract the
   reusable Rust analyzer rules and any generalized helper framework into
   standalone `sc-lint`

Acceptance:
- each family has one concrete implementation home and one concrete integration
  point
- every family is classified as either:
  - reusable and intended for later `sc-lint` migration, or
  - ATM-local and retained in repository-local lint glue
- no family is left as “QA-only tribal knowledge”
- sprint plan: `docs/phase-R/sprint-R19.md`

### R.20 Daemon Partitioning And Enforcement Hardening

Status:
- in review on `feature/pR-s20-daemon-partitioning`

- review the post-`PR #200` integrated daemon state on `integrate/phase-R`
- define the daemon-private partition plan for exactly these eight partitions:
  - `ownership`
  - `server_runtime`
  - `request_runtime`
  - `runtime_status`
  - `peer_transport`
  - `watch_runtime`
  - `reconcile_runtime`
  - `notification_runtime`
- tighten daemon architecture, requirements, and boundaries so the partitioned
  design is explicit and enforceable
- run a repeated plan-hardening loop over code and docs until the daemon
  planning set is internally consistent and specific enough for a production
  cleanup sprint
- sprint plan: `docs/phase-R/sprint-R20.md`

## 6. Working Rule

Phase R does not advance by ad hoc implementation.

Required order:
1. design decision
2. boundary record
3. architecture/requirements/ADR alignment
4. lint/parser support
5. implementation skeleton
6. feature behavior
