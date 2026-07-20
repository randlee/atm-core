---
id: AH.1
title: atm-core Session-ID Protocol + Query Surface
status: planned
branch: feature/pAH-s1-session-id-protocol
worktree: ../atm-core-worktrees/feature/pAH-s1-session-id-protocol
target: develop
---

# Sprint AH.1 — atm-core Session-ID Protocol + Query Surface

```yaml
plan_type: sprint_plan
phase: AH
sprint: AH.1
worktree: ../atm-core-worktrees/feature/pAH-s1-session-id-protocol
branch: feature/pAH-s1-session-id-protocol
status: planned
estimated_scope: medium
```

## Goal

Add `session_id` as a first-class field on the ATM durable message model and
expose session-scoped and peer-scoped `atm read` query surfaces. Phase AH
depends on this field being stable before any downstream sprint can begin.

The session_id field is:

- optional on send; when absent, the atm-daemon auto-assigns a unique
  durable id
- a stable opaque identifier on the stored message; receivers and senders
  round-trip it unchanged
- a queryable column: `atm read` by defaults scopes to the current
  `HERMES_SESSION_KEY`, `--agent <name>` scopes to a peer across sessions,
  `--session-id <id>` scopes to an explicit id

## Hard Dependencies

- atm-core 1.3.1+ release baseline is the starting point; Phase AF/AG work is
  already merged to `develop`
- the atm-daemon's mailbox schema is the only schema that needs to change
- no atm-graft surface change is permitted; AH.1 only extends the durable
  message + query path

## Exact Targets

- `crates/atm-core/src/mailbox/schema.rs` (or wherever the mailbox DDL lives
  on 1.3.1) — schema migration for nullable `session_id` column, backfill
- `crates/atm-core/src/mailbox/queries.rs` — session-scoped and
  peer-scoped read paths
- `crates/atm-core/src/send.rs` — propagate `session_id` through
  `SendRequest` and `MessageType`
- `crates/atm-core/src/types.rs` — `MessageType::session_id` pass-through
  on delivery
- `crates/atm-core/src/cli/send.rs` — add `atm send --session-id <id>`
  (optional)
- `crates/atm-core/src/cli/read.rs` — add `--agent <name>` query mode;
  default mode reads `HERMES_SESSION_KEY` env and scopes accordingly
- migration + backfill test covering existing rows getting a stable id

## Deliverables

Every listed deliverable is expected to land at a production-ready level for
the scope this sprint claims.

- schema change adding nullable `session_id` to the mailbox row
- automatic backfill that assigns a stable idempotency-style id to existing
  rows on migration (so older rows remain queryable but do not get `NULL`)
- `MessageType` carries the field in memory and over the daemon-client
  boundary
- daemon auto-assigns when `atm send` omits session_id
- `atm send --session-id <id>` (optional; binding code will auto-attach)
- `atm read` default mode picks up `HERMES_SESSION_KEY` and scopes results
- `atm read --agent <name>` query mode; returns all messages with a
  specific peer across all sessions
- `atm read --session-id <id>` explicit id query
- `atm send` / `atm read` round-trip integration tests for:
  - send with explicit session_id, read with session scope
  - send without session_id, read with session scope
  - `atm read --agent <peer>` across a multi-turn conversation
  - `atm read --session-id <id>` explicit mode

## Required Work

### Schema migration

The atm-daemon mailbox table gains a nullable `session_id TEXT` column.

- a migration module runs on daemon startup; backfill assigns a stable
  unique id to existing rows (idempotency-safe so the migration is
  re-runnable)
- the migration runs exactly once; if it fails, the daemon refuses startup
  until the operator addresses it

### Send path

`SendRequest` gains an optional `session_id: Option<SessionId>` field.

- If the sender sets it, the daemon stores it unchanged
- If the sender omits it, the daemon generates a durable id (uuid-v4 style)
  and stores + returns it in the send response

### Delivery path

`MessageType` carries `session_id: SessionId` (always non-null on receipt;
either sender-supplied or daemon-assigned).

- The daemon-client protocol surfaces the field on delivery
- atm-graft consumers see it via `PostSendHookEvent.session_id`

### Query path

`atm read` behavior:

- default mode reads `HERMES_SESSION_KEY` from the caller's environment;
  results are scoped to rows whose `session_id` matches
  `HERMES_SESSION_KEY` stripped of its namespace prefix, OR if
  `HERMES_SESSION_KEY` is unset, fall back to current behavior
- `--agent <name>` ignores session scope entirely and returns messages
  involving that peer across all sessions
- `--session-id <id>` scopes to an explicit id
- explicit `--session-id` overrides ambient
- `--team` / `--as` compose with session-scope as before

### Boundary And Type Contract

Illustrative Rust signatures:

```rust
// mailbox storage
pub struct MailboxRow {
    pub message_id: MessageId,
    pub sender: AgentName,
    pub recipient: AgentName,
    pub team: TeamName,
    pub body: MessageBody,
    pub session_id: SessionId,       // always set; daemon-assigned when the sender omitted it
    pub requires_ack: bool,
    pub created_at: DateTime<Utc>,
    // ...
}

// query
pub enum ReadScope {
    Session(SessionId),
    Agent(AgentName),
    SessionId(SessionId),
}

pub trait MailboxStore {
    fn read(&self, query: ReadQuery) -> Result<Vec<MailboxRow>, AtmError>;
}

// send
pub struct SendRequest {
    // ...existing fields...
    pub session_id: Option<SessionId>,   // sender-supplied; daemon fills if absent
}

// delivery
pub struct MessageType {
    // ...existing fields...
    pub session_id: SessionId,           // always non-null on delivery
}
```

These names are illustrative; the sprint requires equivalent explicit
ownership boundaries so session_id does not leak into unrelated boundary
code as a hidden coupling.

## Non-Closure

This sprint does not:

- implement the Python binding (AH.2)
- implement the Hermes webhook adapter change (AH.3)
- implement any Hermes session routing (AH.3)
- implement launchd bridge processes (AH.4)

## Acceptance Criteria

- the schema migration is concrete enough for a dev sprint to implement
  directly against 1.3.1 schema
- the query modes (session-scoped default, peer-scoped, explicit id-scoped)
  are each represented as distinct code paths, not overloads of the same
  surface with ambiguous precedence
- `atm send` without `--session-id` still works; missing session_id is not
  a CLI error
- `atm read` with `HERMES_SESSION_KEY` unset works (falls back to current
  behavior); missing env var is not an error
- round-trip tests prove session_id is preserved end-to-end
- no atm-graft surface changes are introduced

## Required Validation

- `cargo build --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace -- -D warnings`
- explicit migration test: start daemon on existing state, verify backfill,
  restart, verify no-op
- explicit query-mode tests: session-scoped, agent-scoped, explicit id
- `git diff --check`
