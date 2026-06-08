# Phase AC `atm-storage` Contract Candidate

## Goal

Provide a first-pass candidate contract for `AC.1` so the shared storage crate
can be implemented from an explicit baseline instead of re-deriving trait and
type shape from scattered notes.

This is an `AC.0` planning collateral artifact. `AC.1` may refine it, but may
not expand beyond this shape without explicit justification.

## Candidate Core Traits

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

Initial notification-policy note:
- `StorageNotifier` intentionally does not include `task_changed`
- task mutations are notification-free in the first `atm-storage` contract
- any future task-notification expansion requires explicit ADR-level review

## Candidate Canonical Shared Types

Minimal first-pass shared types:

- `Message`
- `MessageKey`
- `MessageQuery`
- `RosterMember`
- `RosterSnapshot`
- `MessageReceivedEvent`
- `RosterChangedEvent`

Expected supporting newtypes likely reused rather than recreated:

- `AgentName`
- `TeamName`
- `TaskId`
- `AtmMessageId`
- timestamps / ack state newtypes already accepted elsewhere

`MessageKey` must remain aligned with `ADR-012` one-message-identity rules.
`AC.1` must treat `MessageKey` as a wrapper around `AtmMessageId`, not as a
new parallel identity system.

## Current Source Mapping

Candidate source mapping from current shapes:

| Future Shared Type | Current Inputs |
| --- | --- |
| `Message` | `schema::MessageEnvelope`, `boundary::MailStoreMessageRecord`, logical delivery message layers |
| `MessageKey` | `boundary::MessageKey` |
| `MessageQuery` | mailbox metadata query wrappers, read/list/clear selection inputs |
| `RosterMember` | `boundary::RosterMemberRecord`, `boundary::ProjectionRosterMember` |
| `RosterSnapshot` | `boundary::ProjectionRoster`, roster load/replace wrapper payloads |

## Explicit Non-Goals For AC.1

`AC.1` must not pull these into the shared core contract as-is:

- `*Request` / `*Response` wrapper families
- file-path and lock helper structs
- JSON repair / salvage helper types
- SQLite transaction helper types
- daemon/runtime composition bundles
- speculative task-store traits, records, and SQLite task persistence shapes

## Deferred Task-Storage Rule

Task storage is not part of the initial `atm-storage` contract.

Current `TaskStore` code is treated as speculative pre-design surface rather
than as approved baseline. Phase `AC` must not preserve it by forcing a
premature shared `TaskStore` contract.

If task storage is approved later, the required starting point is:

- canonical Claude-code task storage behavior
- validation against the Claude schema and its Pydantic models
- only then any SQLite synchronization against that canonical model

## Candidate Capability Traits

These are permitted only if needed and must stay small:

- `StorageHealth`
- `ReplayStore`
- `RepairableStorage`
- `TransactionalStorage`

If more than these are needed, `AC.1` must justify the expansion against
`ADR-018`.

## AC.1 Review Questions

Before `AC.1` closes, it must answer:

- which current query wrappers collapse into `MessageQuery`
- whether replay state belongs in the core contract or only in a capability
  trait
- which current roster projection fields are canonical shared fields versus
  Claude-backend-only projection details
- whether any temporary `MailStore` compatibility seam survives inside the
  implementation phase at all; the planning default is deletion at `AC.1`

Resolved during planning hardening:

- `delete_message` is part of the initial core CRUD contract
- `StorageNotifier` remains message/roster-only in the initial contract; task
  mutations are intentionally notification-free unless a later ADR changes that
- task storage itself is deferred out of the initial `atm-storage` contract
