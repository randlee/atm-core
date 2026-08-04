---
id: AJ.1
title: SessionId Type And Protocol Extensions
status: complete
branch: feature/pAJ-s1-session-id-and-protocol
worktree: ../atm-core-worktrees/feature/pAJ-s1-session-id-and-protocol
target: integrate/phase-aj
---

# Sprint AJ.1 — SessionId Type And Protocol Extensions

## Goal

Introduce the canonical `SessionId` core type and extend the heartbeat
protocol structs so every later sprint has one shared vocabulary for session
identity.

## Hard Dependencies

- `integrate/phase-ai-31-33 @ 150391ecdf2e003185bff7d78427cd21509a7981`
- Phase AI merged to `develop`; phase-entry reconciliation recorded the
  post-merge SHA and reviewed all AJ exact targets for baseline drift
- `integrate/phase-aj`, cut from that reconciled post-merge `develop` SHA
  before AJ.1 implementation dispatch
- `docs/plans/phase-aj/plan-phase-aj.md`
- `docs/plans/phase-aj/phase-aj-research.md`
- `crates/atm-core/src/protocol.rs` at the recorded Phase AJ baseline

## Dependency Relation

- `must_follow` the recorded `integrate/phase-ai-31-33 @ 150391ec` planning baseline;
  AJ.1 may begin only after the Phase-AI reconciliation gate creates its
  post-merge-`develop` implementation target.
- No AJ sprint is `parallel_safe`: AJ.2–AJ.10 consume this public `SessionId`
  contract. AJ.2 begins immediately when AJ.1's development head is merged
  forward into its branch; it does not wait for AJ.1 QA. AJ.2 must repeat that
  merge-forward before every dev/fix round and cannot complete its PR first.
- On AJ.1 development-head push, AJ.2 begins immediately by merging
  AJ.1 → AJ.2; AJ.2 must complete that merge before any dev/fix round and does
  not wait for AJ.1 QA.

## Exact Targets

- `crates/atm-core/src/types.rs`
- `crates/atm-core/src/protocol.rs`
- `crates/atm-daemon/src/runtime_health.rs` (mechanical `session_id: None` literals)
- `crates/atm-daemon/src/runtime_status_cache.rs` (mechanical `session_id: None` literals)
- `crates/atm-daemon/src/tests.rs` (mechanical `session_id: None` literals)

## Why `SessionId` Is A Newtype

`SessionId` is a newtype wrapper around `String` rather than a plain
`String` for type safety and to prevent conflation with the other
string-like identifiers already moving through the protocol
(`TeamName`, `AgentName`, message bodies, identity strings). Function
signatures that take `SessionId` cannot accidentally accept a member
name or an arbitrary payload string. Its smart constructor centralizes AJ's
content rules: blank/whitespace normalizes to absent, and a non-blank value is
limited to 256 UTF-8 bytes before it can enter request, cache, or audit-log
state. Serde retains the string wire representation; the optional field
deserializer maps legacy blank values to `None` rather than creating an invalid
value.

## Interfaces To Add Or Modify

- New transparent core type in `crates/atm-core/src/types.rs`:
  ```rust
  pub struct SessionId(String);
  ```
  It derives `Serialize`, `Deserialize`, `Clone`, `Debug`, `PartialEq`, `Eq`,
  and `Hash`; it implements `Display` and `AsRef<str>`. Its only public
  construction API is `SessionId::new(value) -> Result<SessionId, SessionIdError>`;
  `SessionId::parse_optional` and the optional wire deserializer normalize
  whitespace-only input to absent, while a value over 256 UTF-8 bytes returns
  `SessionIdError::TooLong`.
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
- The optional request/response field uses
  `deserialize_optional_session_id`, which maps legacy blank/whitespace wire
  input to `None` before construction. It therefore neither clears a known
  session nor appears in a roster projection.
- The canonical import is `atm_core::types::SessionId`; AJ.1 does not add a
  top-level or prelude re-export.

## Deliverables

- `SessionId` newtype exists, derives the listed traits, and round-trips
  through `serde_json` without loss for valid values
- Tests prove whitespace maps to absent, a 256-byte value succeeds, and a
  257-byte value returns `SessionIdError::TooLong`; no direct `From<String>`
  or `From<&str>` construction bypass remains
- Heartbeat request/response structs serialize with `session_id` present
  when `Some`, omit the field entirely when `None`
- The response behavior is the authoritative post-update cached-value contract
  defined in **Interfaces To Add Or Modify** above: a request containing `None`
  may return the prior known `Some` value after AJ.5, while AJ.1 itself adds no
  cache behavior.
- Older payloads missing `session_id` deserialize with `session_id: None`
  and an existing reader ignores the additive field. Lossless re-serialization
  through an older reader is not required.
- `RuntimeMemberState`, `RuntimeStatusSnapshot`, and `HeartbeatActivity`
  are unchanged in this sprint

## Acceptance Criteria

- `SessionId` is the canonical opaque core newtype for session identity; old
  payloads deserialize with `None` and no cache behavior exists yet.
- AJ.1 must_follow Phase AI cutover; the dispatch records the phase target SHA.

## Required Validation

- `cargo build -p agent-team-mail-core`
- `cargo clippy -p agent-team-mail-core --all-targets -- -D warnings`
- `cargo test -p agent-team-mail-core`
- New unit tests in `crates/atm-core/src/types.rs` and `protocol.rs`:
  - serde round-trip on `Some` and `None`
  - missing field deserializes to `None`
  - `Display` preserves the validated string representation
- `rg -n "session_id" crates/atm-core/src/protocol.rs` shows both new
  fields with the required serde attributes
- `git diff --check`
