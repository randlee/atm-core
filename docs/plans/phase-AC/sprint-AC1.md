# AC.1 `atm-storage` Contract And Canonical Domain Types

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.1
worktree: ../atm-core-worktrees/feature/pAC-s1-atm-storage-contract-and-canonical-types
branch: feature/pAC-s1-atm-storage-contract-and-canonical-types
status: complete
estimated_scope: large
```

## Goal

Create `crates/atm-storage` as the small audited storage contract crate and
define the canonical shared domain structs used by both storage and RPC bodies.

## Scope Summary

This sprint creates the shared contract only. It does not extract Claude or
SQLite implementations yet. The work is to define a small semantic API and a
small shared data model that later backends can implement.

Production-ready commitment:
- every deliverable listed in this sprint is expected to land at a
  production-ready level for the contract-definition scope this sprint claims;
  boundary-only, test-only, or placeholder contract closure is not accepted
- task storage is explicitly out of scope for this sprint and must not be
  smuggled into `atm-storage` as speculative compatibility surface

Primary closure rule:
- `AC.1` is the primary closure sprint for the shared `atm-storage` contract,
  canonical shared types, and the storage wrapper families being replaced
- later sprints may migrate consumers or verify deletion, but they do not
  reopen the contract/type decisions made here

Why this sprint is not split:
- `AC.1` intentionally couples crate creation, canonical-type freeze, and the
  first wrapper-family deletion pass so the repo does not keep two competing
  shared-storage contracts alive in parallel
- the mechanical deletion work in this sprint is allowed only behind the
  explicit compilation bridge defined below

## Governing Sources

- `docs/plans/phase-AC/plan-phase-AC.md`
- the Phase `AC` ADR created in `AC.0`
- `crates/atm-core/src/boundary/mail.rs`
- `crates/atm-core/src/boundary/store.rs`
- `crates/atm-core/src/boundary/runtime.rs`

## Prerequisites

- `AC.0`

## Out Of Scope

- no backend extraction yet
- no daemon/runtime refactor yet
- no SQL query/index capability work beyond trait design

## Deliverables

- `crates/atm-storage` exists and exports the shared storage contract

- The initial core trait set is small and semantic:

  ```rust
  pub trait MessageStore {
      fn save_message(&self, message: &Message) -> Result<(), AtmError>;
      fn load_message(&self, key: &MessageKey) -> Result<Option<Message>, AtmError>;
      fn list_messages(&self, query: &MessageQuery) -> Result<Vec<Message>, AtmError>;
      fn delete_message(&self, key: &MessageKey) -> Result<(), AtmError>;
  }

  pub trait RosterStore {
      fn load_roster(&self, team: &TeamName) -> Result<RosterSnapshot, AtmError>;
      fn save_roster(&self, roster: &RosterSnapshot) -> Result<(), AtmError>;
      fn list_teams(&self) -> Result<Vec<TeamName>, AtmError>;
  }

  pub trait StorageNotifier {
      fn message_received(&self, event: &MessageReceivedEvent) -> Result<(), AtmError>;
      fn roster_changed(&self, event: &RosterChangedEvent) -> Result<(), AtmError>;
  }
  ```

- `StorageNotifier` is intentionally limited to `message_received` and
  `roster_changed` in the initial contract; task mutations are notification-free
  unless a later ADR deliberately adds a task-notification surface

- Shared canonical structs exist for:
  - `Message`
  - `MessageKey`
  - `MessageQuery`
  - `RosterMember`
  - `RosterSnapshot`
  - notification event structs

- The crate does **not** carry:
  - backend-specific path/lock/file structs
  - request/response-per-method DTO pairs
  - JSON- or SQLite-shaped helper types

- Capability traits are explicit and capped. If more than four capability
  traits are needed, the sprint must update the ADR before proceeding.

## Ledger-Driven Type Work

`AC.1` owns the canonical shared contract and the first major type collapse.
The default expectation is that these current surfaces do **not** survive in
their present form:

Move into `atm-storage` as canonical shared types or small semantic helpers:

- `MessageKey`
- `TaskState` and `AckTransition` as ack-state semantic helpers, not as part
  of the deferred `TaskStore` family
- `MessageFingerprint` if still justified after the contract pass
- canonical replacements for:
  - `MailStoreMessageRecord` -> `Message`
  - `MailStoreMailboxMetadataRow` / `MailStoreMailboxMetadataCounts` -> `MessageQuery` helpers
  - `RosterMemberRecord` -> `RosterMember`
  - roster snapshot wrappers -> `RosterSnapshot`

Delete or collapse during `AC.1` rather than carrying them forward:

- all `MailStore*Request` / `MailStore*Response` wrappers except the temporary
  `MailStoreBootstrap*` compile bridge owned by `AC.4`
- all `RosterStore*Request` / `RosterStore*Response` wrappers
- `MailStoreRequest` / `MailStoreResponse`
- `RosterStoreRequest` / `RosterStoreResponse`

Replace old storage traits in this sprint:

- `MailStore` -> `MessageStore`
- `RosterStore` -> `RosterStore` in `atm-storage`

Task-storage deferral rule:

- `TaskStore`, `Task`, `TaskKey`, and `TaskQuery` are not part of the initial
  `atm-storage` contract
- existing `TaskStore*` wrappers and SQLite task-store code are speculative
  surface and must not be re-homed into `atm-storage`
- any future task-storage line must start from canonical Claude-code task
  schema plus Pydantic validation rather than inheriting these speculative
  shapes

Must remain outside `atm-storage` even if they still exist elsewhere:

- `AtmProtocol`
- `ClientTransport`
- `ServerTransport`
- `RequestDispatcher`
- `NotificationSink`
- `StatusSource`
- `WatchEventSource`
- `ReconcileCoordinator`

## Execution Checklist

Implementation order for `AC.1`:

1. Scaffold `crates/atm-storage` with the smallest possible public surface.
2. Freeze the canonical shared type list before moving any trait signatures:
   - `Message`
   - `MessageKey`
   - `MessageQuery`
   - `RosterMember`
   - `RosterSnapshot`
   - notification event types
3. Reuse existing semantic seeds where possible rather than cloning them:
   - `schema::AtmMessageId`
   - `TeamName`
   - `AgentName`
   - `TaskId`
4. Replace the old store traits with the new CRUD traits in `atm-storage`.
5. Keep the workspace buildable while the consumer cutover is incomplete:
   - a deprecated compile bridge may remain temporarily inside `atm-core`
   - that bridge may forward or alias legacy `MailStore` / `RosterStore`
     names to the new `atm-storage` contract
   - the bridge is not authoritative API surface and must not gain new
     semantics
   - `MailStoreBootstrap*` may remain only as part of that bridge until `AC.4`
     removes the backend bootstrap seam
6. Delete the wrapper families instead of re-homing them, except for the
   temporary `MailStoreBootstrap*` bridge noted above.
7. Update the boundary TOMLs so ownership moves from `atm-core` to `atm-storage`.

Proof this sprint must leave behind:

- `atm-storage` is small enough to audit directly
- the old boundary traits are no longer the authoritative shared contract
- no surviving shared public type is backend-shaped or request/response-shaped
- `cargo build --workspace` still passes because any temporary legacy trait
  names are carried only by the deprecated compile bridge
- wrapper-family deletion is committed as an `AC.1` closure obligation and is
  not deferred to `AC.6`; later sprints may only verify that no deleted family
  reappeared

## Acceptance Criteria

- `atm-storage` exposes a small audited contract rather than a lifted copy of current boundary DTOs
- the canonical shared structs are suitable for both RPC bodies and storage
- no core trait name or method is backend-specific
- the crate graph remains `atm-core -> atm-storage`, not the reverse
- `MessageKey` wraps `AtmMessageId` per `ADR-012` rather than introducing a divergent message-identity contract
- `atm-storage` is the only authoritative shared storage contract after
  `AC.1`; any surviving legacy `MailStore` / `RosterStore` names are
  compile-bridge shims only and must be deleted by `AC.4`
- no `*Request` / `*Response` storage wrapper families are recreated inside `crates/atm-storage`
- `RosterMemberRecord`, `ProjectionRosterMember`, and `ProjectionRoster` are not copied into `atm-storage` unchanged
- task mutations are intentionally notification-free in the initial contract;
  `StorageNotifier` does not silently grow a `task_changed` event in this sprint
- `atm-storage` does not introduce `TaskStore`, `Task`, `TaskKey`, or
  `TaskQuery` as speculative first-pass contract surface

## Required Validation

- `cargo build --workspace`
- `cargo test -p atm-storage`
- `cargo clippy -p atm-storage -- -D warnings`
- `cargo tree -p atm-storage`
- `git diff --check`
- `rg -n "MailStore(Query|Transaction|Upsert|Load|Record|Health|Request|Response)|RosterStore(Replace|Load|Query|Health|List|Request|Response)" crates/atm-storage crates/atm-core/src/boundary -S`
- `rg -n "MailStore|RosterStore" crates/atm-storage crates/atm-core/src/boundary -S`
- verify `atm-core` is not present in the transitive dependency tree for `atm-storage`

## Required Document Updates

- `docs/plans/phase-AC/sprint-AC1.md`
- `docs/plans/phase-AC/readiness.md`
- `docs/plans/phase-AC/issues.md`
- `docs/plans/phase-AC/plan-phase-AC.md`
- `docs/project-plan.md`
- create `boundaries/atm-storage/` TOML records for:
  - `MessageStore`
  - `RosterStore`
  - `StorageNotifier`
- each `boundaries/atm-storage/` TOML record must include `allowed_dependents = ["atm-core", "atm-storage-claude", "atm-storage-rusqlite"]` so `lint_boundaries.py` can enforce the ownership topology
- update existing `boundaries/atm-core/` store TOMLs to reflect the ownership move into `atm-storage`
- verify `lint_boundaries.py` accepts the new ownership topology before sprint closure

## Risks And Watchouts

- if the contract copies the current boundary request/response volume, the phase has failed
- if the shared structs are still transport-shaped rather than semantic, `AC.5` will stall
- if this sprint leaves both old and new shared contracts standing indefinitely, `AC.4` and `AC.6` will false-close

## Closure Note

- `AC.1` landed `crates/atm-storage` with canonical message/roster contract
  traits, canonical shared message/roster structs, notification events, and an
  `atm-core` compile bridge that keeps the workspace buildable while later
  consumer-migration sprints close the legacy surfaces
