---
title: AI.36 graft receiver ownership
status: complete
branch: feature/pAI-s36-graft-receiver-ownership
target: integrate/phase-ai-31-33
depends_on: AI.35
---

# AI.36 — Graft receiver ownership

```yaml
plan_type: sprint_plan
phase: AI
sprint: AI.36
worktree: feature/pAI-s36-graft-receiver-ownership
branch: feature/pAI-s36-graft-receiver-ownership
status: complete
estimated_scope: small cross-platform Rust lifecycle change
```

## Goal

Make one live graft receiver the unambiguous owner of each canonical
`(graft_root, team, agent)` endpoint. A restart may reclaim a dead owner, but
two live profiles must never overwrite or delete one another's endpoint.

## Scope Summary

This sprint changes receiver ownership only. It does not add a mailbox queue,
retry delivery, multi-chat routing, or Hermes-specific steering. The current
single endpoint path remains `.atm/graft/<team>/<agent>.json`; `ChatId` is
recorded for validation/observability only and does not change that path.

## Governing Requirements

- `REQ-GRAFT-RUNTIME-002`
- `REQ-GRAFT-NOTIFY-002`
- `REQ-CORE-GRAFT-001`
- ADR-037 address identity and `ChatId` preservation

## Governing ADRs

- ADR-039 — Python Graft Host Binding
- ADR-043 — Hermes Graft Wake-up Ownership and Recovery

## Governing Boundaries

- `docs/atm-graft/boundaries.md`: Session Runtime Consumer and Post-Send
  Notification Transport
- `atm-core::graft::GraftReceiverListener` owns record format/publication;
  `atm-graft::GraftSession` owns lifecycle.

## Prerequisites

- AI.34's canonical graft-root resolver is merged; publisher and daemon use
  the same root.

## Hard Dependencies

- This sprint blocks AI.37 and AI.38 because recovery/steer cannot be reliable
  while a second profile can steal or erase the receiver endpoint.

## Non-Goals

- No daemon-owned session registry, SQLite table, durable nudge queue, or
  message retry state.
- No endpoint fan-out or multiple active chat sessions for one agent.
- No normal-message or steer injection behavior.

## Sub-Tasks

### 1. Version and record contract

Development work:

1. First commit sets all releasable Rust assemblies to `1.4.0-beta.36` and
   Python packaging metadata to the equivalent PEP 440 `1.4.0b36`.
2. In `crates/atm-core/src/graft.rs`, extend the private endpoint-record
   format with a random `owner_generation` and the optional owning `ChatId`.
   Bump `GRAFT_RECEIVER_RECORD_SCHEMA_VERSION`; old records fail closed rather
   than being guessed as live owners.
3. Add a private ownership guard acquired before record publication. Reuse the
   repository's existing `fs2::FileExt::try_lock_exclusive()` precedent from
   `crates/atm-daemon/src/host_ownership.rs`; `fs2` maps this to advisory file
   locking on Unix and `LockFileEx` on Windows. Do not use a create-only
   sentinel that survives a crashed process.

Required shape:

```rust
struct ReceiverOwnershipGuard { /* exclusive lock held for listener lifetime */ }

struct GraftReceiverEndpointRecord {
    schema_version: u8,
    owner_generation: String,
    owner_chat_id: Option<ChatId>,
    loopback: SocketAddr,
    capability_base64url: String,
}
```

Required tests:

- record decoding rejects the old schema and a malformed generation;
- one owner publishes a record containing its generation and optional chat ID.

### 2. Acquire, close, and reclaim lifecycle

Development work:

1. Change `GraftReceiverListener::bind` to acquire the guard before binding
   and publishing. A conflicting live acquisition returns a typed
   `GraftReceiverAlreadyActive` error naming root/team/agent; it must not bind
   a replacement socket or write the record.
2. Change `Drop`/close cleanup to read the current record and unlink only when
   `owner_generation` equals the listener generation. Never blindly remove the
   path.
3. Thread the caller's optional `ChatId` from `GraftSessionOptions` into the
   receiver record. A live owner with a different chat ID still conflicts; AI.36
   does not choose a multi-session routing policy.

Required tests:

- two concurrent activations of the same identity: first remains reachable,
  second fails, and record bytes are unchanged;
- an old listener's cleanup cannot delete a manually published successor
  generation;
- a short-lived child process acquires then exits without close; a parent can
  subsequently acquire the same guard on macOS, Linux, and Windows;
- distinct agents or teams under the same root can listen concurrently.

### 3. Boundary and observability closure

Development work:

1. Emit structured activation/conflict/reclaim/compare-remove outcomes through
   existing graft observability; never log capability material.
2. Update Python snapshot/error translation only as needed to surface the
   typed conflict without parsing strings.
3. Add a structural test/gate that the endpoint record is written only by
   `GraftReceiverListener` and receiver ownership is not reimplemented in the
   Python bridge.

Required tests:

- Python activation reports the typed conflict;
- a source scan rejects a second record publisher or direct record unlink
  outside the owner implementation.

## Split Recommendation

Do not combine this with recovery counts or Hermes injection. Receiver lease
correctness is independently production-ready and its cross-platform failure
model deserves its own closure.

## Acceptance Criteria

1. Exactly one live receiver owns a canonical root/team/agent endpoint.
2. A second live activation changes neither socket nor endpoint record and
   returns a typed error.
3. A dead owner is reclaimable without manual filesystem cleanup.
4. Old cleanup cannot remove a successor's record.
5. `ChatId` is preserved as owner metadata without adding multi-session
   routing.

## Required Validation

```text
cargo test -p agent-team-mail-core graft -- --nocapture
cargo test -p atm-graft -- --nocapture
cargo test -p atm-graft-python -- --nocapture
just lint
just test
```

Run the ownership subprocess tests on macOS, Linux, and Windows CI.

## Required Document Updates

- `docs/atm-graft/requirements.md`
- `docs/atm-graft/boundaries.md`
- ADR-043 status/evidence note

## Risks And Watchouts

- Do not treat an endpoint record's existence as ownership; only the held OS
  lock is authoritative.
- Do not make the owner generation a routing or daemon session token.
- Keep the record capability secret and owner-readable as today.
