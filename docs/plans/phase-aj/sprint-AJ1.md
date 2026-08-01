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

- `integrate/phase-AJ` at the Phase AJ entry-gate SHA
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-core/src/protocol.rs` at the recorded Phase AJ baseline

## Dependency Relation

- `must_follow` the Phase AI cutover because AJ's protocol baseline is created
  only at that recorded `develop` SHA.
- No AJ sprint is `parallel_safe`: AJ.2–AJ.6 consume this public `SessionId`
  contract. They may begin after this development commit is pushed, but must
  merge AJ.1 forward before every round and cannot complete their PR first.

## Exact Targets

- `crates/atm-core/src/types.rs`
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

- New transparent core type in `crates/atm-core/src/types.rs`:
  ```rust
  pub struct SessionId(String);
  ```
  It derives `Serialize`, `Deserialize`, `Clone`, `Debug`, `PartialEq`, `Eq`,
  and `Hash`; it implements `Display`, `From<String>`, `From<&str>`, and
  `AsRef<str>`.
- `TeamMemberHeartbeatRequest` gains
  `pub session_id: Option<SessionId>` with
  `#[serde(default, skip_serializing_if = "Option::is_none")]`
- `TeamMemberHeartbeatResponse` gains
  `pub session_id: Option<SessionId>` with the same serde attributes.
- **Response semantics (authoritative).** Once AJ.5's cache merge exists, the
  heartbeat response returns the post-update cached session value, not simply
  the request value. AJ.1 defines that contract where it introduces the wire
  field; AJ.5 implements it and does not redefine it.
- `session_id` is transient observation metadata. AJ.1 must not add it to a
  mail row, mail payload, or session-history table.
- A blank `SessionId` is wire-compatible but is normalized to absent by AJ.4's
  cache merge. It therefore neither clears a known session nor appears in a
  roster projection.
- `SessionId` re-exported from `atm_core::prelude` (or the existing
  top-level re-export module)

## Deliverables

- `SessionId` newtype exists, derives the listed traits, and round-trips
  through `serde_json` without loss
- Heartbeat request/response structs serialize with `session_id` present
  when `Some`, omit the field entirely when `None`
- The future heartbeat response contract is explicitly post-update cached value,
  so a request containing `None` may return the prior known `Some` value after
  AJ.5; AJ.1 itself introduces no cache behavior.
- Older payloads missing `session_id` deserialize with `session_id: None`
  and an existing reader ignores the additive field. Lossless re-serialization
  through an older reader is not required.
- `RuntimeMemberState`, `RuntimeStatusSnapshot`, and `HeartbeatActivity`
  are unchanged in this sprint

## Acceptance Criteria

- `SessionId` is the sole opaque core newtype; old payloads deserialize with
  `None` and no cache behavior exists yet.
- AJ.1 must_follow Phase AI cutover; the dispatch records the phase target SHA.

## Required Validation

- `cargo build -p atm-core`
- `cargo clippy -p atm-core --all-targets -- -D warnings`
- `cargo test -p atm-core session`
- New unit tests in `crates/atm-core/src/types.rs` and `protocol.rs`:
  - serde round-trip on `Some` and `None`
  - missing field deserializes to `None`
  - `Display` and `From<&str>` produce identical strings
- `rg -n "session_id" crates/atm-core/src/protocol.rs` shows both new
  fields with the required serde attributes
- `git diff --check`
