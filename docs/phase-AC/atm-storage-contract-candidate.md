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

## Candidate Canonical Shared Types

Minimal first-pass shared types:

- `Message`
- `MessageKey`
- `MessageQuery`
- `RosterMember`
- `RosterSnapshot`
- `Task`
- `TaskKey`
- `TaskQuery`
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
| `RosterMember` | `boundary::RosterMemberRecord`, `boundary::ClaudeCodeRosterMember` |
| `RosterSnapshot` | `boundary::ClaudeCodeTeamRoster`, roster load/replace wrapper payloads |
| `Task` | `boundary::TaskStoreTaskRecord`, `boundary::TaskStoreTaskMetadata` |
| `TaskKey` | current task id / task-key lookup inputs |
| `TaskQuery` | task metadata query wrappers |

## Explicit Non-Goals For AC.1

`AC.1` must not pull these into the shared core contract as-is:

- `*Request` / `*Response` wrapper families
- file-path and lock helper structs
- JSON repair / salvage helper types
- SQLite transaction helper types
- daemon/runtime composition bundles

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

- which current query wrappers collapse into `MessageQuery` / `TaskQuery`
- whether `delete_message` belongs in the initial core contract or as an
  immediate capability
- whether replay state belongs in the core contract or only in a capability
  trait
- which current roster projection fields are canonical shared fields versus
  Claude-backend-only projection details
- whether any temporary `MailStore` compatibility seam survives inside the
  implementation phase at all; the planning default is deletion at `AC.1`
