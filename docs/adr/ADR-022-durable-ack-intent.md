# ADR-022 — Durable Ack Intent

| Field | Value |
|---|---|
| ID | ADR-022 |
| Status | **Accepted** |
| Date | 2026-07-10 |
| Deciders | Rand Lee |
| Relates to | ADR-012, ADR-019 |
| Supersedes | — |

---

## Context

Before `AD.32`, the retained read path still carried a display-time promotion
shape where showing an unread message could manufacture ack-required state in
memory. That contradicted the accepted ATM ownership model:

- send/ack creation owns message intent
- read/peek observe persisted state only
- ack obligation is sender-owned message data, not a reader-side side effect

The concrete failure mode behind GitHub issue `#500` was that ack-required
state could be reconstructed from read/display behavior instead of being
persisted durably on the message itself.

## Decision

Accept the durable ack intent model for retained ATM message state:

- `requires_ack` is a persisted canonical field on every ATM `InboxMessage`
- only send/ack creation paths may set `requires_ack`
- `atm read` and `atm peek` must never create or promote ack-required state
- durable ack classification is derived from the persisted message only:
  - `NotRequired`
  - `RequiredPending`
  - `RequiredAcknowledged`

The retained compatibility rule for historical rows with no explicit
`requires_ack` field is:

- `requires_ack = true` only when `pending_ack_at.is_some()` and
  `acknowledges_message_id.is_none()`
- `requires_ack = false` otherwise

That rule preserves legitimate historical sender-required messages while
preventing historical ack replies from being reclassified as ack-required on
load.

## Enforcement

The accepted retained enforcement points are:

- `crates/atm-storage/src/schema/inbox_message.rs` is the canonical persisted
  contract owner
- `crates/atm-core/src/read/state.rs::derive_ack_requirement(...)` is the sole
  accepted classifier for durable ack-required state
- `AckActivationMode::PromoteDisplayedUnread` is deleted and must not return
- read/peek mutation paths may mark read state, but they must not synthesize
  `pending_ack_at` or any other ack obligation field

## Consequences

### Positive

- sender intent survives serialization/deserialization explicitly
- read output can no longer drift from persisted ack ownership
- ack replies remain terminal acknowledgements rather than recursively
  ack-required messages

### Negative

- historical compatibility logic now has an explicit branch that must stay
  regression-tested
- touched message fixtures across the workspace must set `requires_ack`
  intentionally instead of relying on older inferred behavior

## Review Conditions

This ADR remains valid only while all of the following stay true:

- sender-owned `requires_ack` remains canonical across CLI, daemon, storage,
  and schema re-exports
- read/peek stay observation-only with respect to ack obligation
- compatibility tests keep covering the historical no-`requires_ack` load path
