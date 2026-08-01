---
id: AJ.1
title: SessionId Type And Protocol Extensions
status: planned
branch: feature/pAJ-s1-session-id-and-protocol
worktree: ../atm-core-worktrees/feature/pAJ-s1-session-id-and-protocol
target: integrate/phase-AJ
---

# Sprint AJ.1 — SessionId Type And Protocol Extensions

## Goal

Introduce the canonical `SessionId` core type and extend the heartbeat
protocol structs so every later sprint has one shared vocabulary for session
identity.

## Hard Dependencies

- `integrate/phase-ai-31-33` checked out as the working baseline
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-core/src/protocol.rs` at the integrate/phase-ai-31-33 baseline

## Exact Targets

- `crates/atm-core/src/session.rs` (new)
- `crates/atm-core/src/lib.rs`
- `crates/atm-core/src/protocol.rs`
- `crates/atm-core/src/api.rs` (re-export check only — `HEARTBEAT_PATH`
  unchanged)

## Why `SessionId` Is A Newtype

`SessionId` is a newtype wrapper around `String` rather than a plain
`String` for type safety and to prevent conflation with the other
string-like identifiers already moving through the protocol
(`TeamName`, `AgentName`, message bodies, identity strings). Function
signatures that take `SessionId` cannot accidentally accept a member
name or an arbitrary payload string. The newtype currently enforces no
content invariants — empty and whitespace strings are accepted, matching
the "no validation of contents" rule in AJ.2 — but it reserves a single
place to add validation later (e.g. charset or length limits) without
changing any call site. Serde round-trips through the inner string, so
the wire format is identical to a plain `String` and full
forward/backward compatibility is preserved.

## Interfaces To Add Or Modify

- New `pub struct SessionId(String)` newtype in
  `crates/atm-core/src/session.rs` with `Serialize`, `Deserialize`,
  `Clone`, `Debug`, `PartialEq`, `Eq`, `Hash`, `Display`, `From<String>`,
  `From<&str>`, and `AsRef<str>`
- `TeamMemberHeartbeatRequest` gains
  `pub session_id: Option<SessionId>` with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`
- `TeamMemberHeartbeatResponse` gains
  `pub session_id: Option<SessionId>` with the same serde attributes.
  **Response semantics (authoritative statement):** the response echoes
  the `session_id` that is now cached after the update is applied — a
  post-update read of `RuntimeStatusCache`, not the value carried in the
  request. This keeps the response consistent with cache state; later
  sprints (AJ.5) reference this contract but do not redefine it.
- `SessionId` re-exported from `atm_core::prelude` (or the existing
  top-level re-export module)

## Deliverables

- `SessionId` newtype exists, derives the listed traits, and round-trips
  through `serde_json` without loss
- Heartbeat request/response structs serialize with `session_id` present
  when `Some`, omit the field entirely when `None`
- Older payloads missing `session_id` deserialize with `session_id: None`
  (forward/backward wire compatibility)
- `RuntimeMemberState`, `RuntimeStatusSnapshot`, and `HeartbeatActivity`
  are unchanged in this sprint

## Required Validation

- `cargo build -p atm-core`
- `cargo clippy -p atm-core --all-targets -- -D warnings`
- `cargo test -p atm-core session`
- New unit tests in `crates/atm-core/src/session.rs`:
  - serde round-trip on `Some` and `None`
  - missing field deserializes to `None`
  - `Display` and `From<&str>` produce identical strings
- `rg -n "session_id" crates/atm-core/src/protocol.rs` shows both new
  fields with the required serde attributes
- `git diff --check`
