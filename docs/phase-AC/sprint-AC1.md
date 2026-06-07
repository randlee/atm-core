# AC.1 `atm-storage` Contract And Canonical Domain Types

```yaml
plan_type: sprint_plan
phase: AC
sprint: AC.1
worktree: ../atm-core-worktrees/feature/pAC-s1-atm-storage-contract-and-canonical-types
branch: feature/pAC-s1-atm-storage-contract-and-canonical-types
status: planned
estimated_scope: large
```

## Goal

Create `crates/atm-storage` as the small audited storage contract crate and
define the canonical shared domain structs used by both storage and RPC bodies.

## Scope Summary

This sprint creates the shared contract only. It does not extract Claude or
SQLite implementations yet. The work is to define a small semantic API and a
small shared data model that later backends can implement.

## Governing Sources

- `docs/plan-phase-AC.md`
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

  pub trait TaskStore {
      fn save_task(&self, task: &Task) -> Result<(), AtmError>;
      fn load_task(&self, key: &TaskKey) -> Result<Option<Task>, AtmError>;
      fn list_tasks(&self, query: &TaskQuery) -> Result<Vec<Task>, AtmError>;
  }

  pub trait StorageNotifier {
      fn message_received(&self, event: &MessageReceivedEvent) -> Result<(), AtmError>;
      fn roster_changed(&self, event: &RosterChangedEvent) -> Result<(), AtmError>;
  }
  ```

- Shared canonical structs exist for:
  - `Message`
  - `MessageKey`
  - `MessageQuery`
  - `RosterMember`
  - `RosterSnapshot`
  - `Task`
  - `TaskKey`
  - `TaskQuery`
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
- `TaskState`
- `AckTransition`
- `MessageFingerprint` if still justified after the contract pass
- canonical replacements for:
  - `MailStoreMessageRecord` -> `Message`
  - `MailStoreMailboxMetadataRow` / `MailStoreMailboxMetadataCounts` -> `MessageQuery` helpers
  - `TaskStoreTaskRecord` + `TaskStoreTaskMetadata` -> `Task`
  - `RosterMemberRecord` -> `RosterMember`
  - roster snapshot wrappers -> `RosterSnapshot`

Delete or collapse during `AC.1` rather than carrying them forward:

- all `MailStore*Request` / `MailStore*Response` wrappers
- all `TaskStore*Request` / `TaskStore*Response` wrappers
- all `RosterStore*Request` / `RosterStore*Response` wrappers
- `MailStoreRequest` / `MailStoreResponse`
- `TaskStoreRequest` / `TaskStoreResponse`
- `RosterStoreRequest` / `RosterStoreResponse`

Replace old storage traits in this sprint:

- `MailStore` -> `MessageStore`
- `TaskStore` -> `TaskStore` in `atm-storage`
- `RosterStore` -> `RosterStore` in `atm-storage`

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
   - `Task`
   - `TaskKey`
   - `TaskQuery`
   - notification event types
3. Reuse existing semantic seeds where possible rather than cloning them:
   - `schema::AtmMessageId`
   - `TeamName`
   - `AgentName`
   - `TaskId`
4. Replace the old store traits with the new CRUD traits in `atm-storage`.
5. Delete the wrapper families instead of re-homing them.
6. Update the boundary TOMLs so ownership moves from `atm-core` to `atm-storage`.

Proof this sprint must leave behind:

- `atm-storage` is small enough to audit directly
- the old boundary traits are no longer the authoritative shared contract
- no surviving shared public type is backend-shaped or request/response-shaped

## Acceptance Criteria

- `atm-storage` exposes a small audited contract rather than a lifted copy of current boundary DTOs
- the canonical shared structs are suitable for both RPC bodies and storage
- no core trait name or method is backend-specific
- the crate graph remains `atm-core -> atm-storage`, not the reverse
- `MessageKey` wraps `AtmMessageId` per `ADR-012` rather than introducing a divergent message-identity contract
- `MailStore` in `atm-core` is deleted in `AC.1` when `MessageStore` lands in `atm-storage`; coexistence beyond `AC.1` is not an accepted outcome
- no `*Request` / `*Response` storage wrapper families are recreated inside `crates/atm-storage`
- `RosterMemberRecord`, `ClaudeCodeRosterMember`, and `ClaudeCodeTeamRoster` are not copied into `atm-storage` unchanged

## Required Validation

- `cargo test -p atm-storage`
- `cargo clippy -p atm-storage -- -D warnings`
- `cargo tree -p atm-storage`
- `git diff --check`
- `rg -n "Request|Response" crates/atm-storage -S`
- `rg -n "MailStore|TaskStore|RosterStore" crates/atm-storage crates/atm-core/src/boundary -S`
- verify `atm-core` is not present in the transitive dependency tree for `atm-storage`

## Required Document Updates

- `docs/phase-AC/sprint-AC1.md`
- `docs/phase-AC/readiness.md`
- `docs/phase-AC/issues.md`
- `docs/plan-phase-AC.md`
- `docs/project-plan.md`
- create `boundaries/atm-storage/` TOML records for:
  - `MessageStore`
  - `RosterStore`
  - `TaskStore`
  - `StorageNotifier`
- each `boundaries/atm-storage/` TOML record must include `allowed_dependents = ["atm-core", "atm-storage-claude", "atm-storage-rusqlite"]` so `lint_boundaries.py` can enforce the ownership topology
- update existing `boundaries/atm-core/` store TOMLs to reflect the ownership move into `atm-storage`
- verify `lint_boundaries.py` accepts the new ownership topology before sprint closure

## Risks And Watchouts

- if the contract copies the current boundary request/response volume, the phase has failed
- if the shared structs are still transport-shaped rather than semantic, `AC.5` will stall
- if this sprint leaves both old and new shared contracts standing indefinitely, `AC.4` and `AC.6` will false-close
