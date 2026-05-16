# ATM Message Schema

## 1. Ownership

This file documents the ATM-owned compatibility fields layered on top of the
Claude Code-native message schema in
[`claude-code-message-schema.md`](./claude-code-message-schema.md).

Ownership rules:

- Claude Code owns the native inbox shape.
- ATM owns only the additive compatibility fields and semantics listed here.
- ATM must not treat Claude JSON as the durable truth for ATM-owned machine
  state.
- Unknown additive fields must be tolerated and preserved.
- Phase `Y` requires one field-justification ledger for every ATM additive
  field that survives on the shared inbox surface.

Enforcement model in this repo:

- `tools/schema_models/atm_message_schema.py`

## 2. Supported Additive Compatibility Fields

The shared Claude inbox surface may contain these ATM additive fields when ATM
needs compatibility with existing Claude-side consumers:

- `message_id`
- `source_team`
- `pendingAckAt`
- `acknowledgedAt`
- `acknowledgesMessageId`
- `parentMessageId`
- `threadMode`
- `taskId`

These fields are compatibility fields only. They are not the durable ATM-owned
source of truth for mailbox state.

Phase `Y` rule:

- field survival decisions must be owned by the relevant event-family state
  machine plus the central delivery-policy coordinator
- fields must not persist merely because scattered command code still reads
  them
- `Y.4` is the sprint that decides which of these fields actually survive

## 3. One Message Identity

ATM uses one logical message identity.

Rules:

- ATM keeps one logical message identity in its own system.
- Claude inbox `message_id` is the shared-wire encoding of that same identity.
- if ATM persists a durable `message_id` column, it stores that same logical
  identity in the compatibility wire form rather than as a second ATM-owned id
- ATM must not persist a second ATM-owned message id under another field name.
- `metadata.atm.messageId` is not part of the approved schema.
- confusing `legacy_*` naming should be removed from the implementation line in
  Phase U; the surviving identifier should be named for what it actually is.

## 4. What Does Not Belong In Claude JSON

The following ATM-owned data must live in SQLite state rather than in shared
Claude JSON:

- read/unread state
- ack-required / acknowledged state
- delete/close state
- expiration state (`expires_at`)
- canonical sender projection
- repair/alert machine metadata

If an older compatibility shape is ever reconsidered, it requires explicit
approval; the active Phase U design preserves no `metadata.atm` namespace.

## 5. Threading And Task Semantics

ATM may continue using these compatibility fields on the shared inbox surface:

- `parentMessageId`
- `threadMode`
- `taskId`

Interpretation rules:

- `threadMode` is limited to the approved product modes:
  - `add-details`
  - `supersede`
- `taskId` is a shared reference value, not proof that ATM owns a full task
  object model in the inbox.

The durable current-state meaning of those fields belongs in SQLite-backed read
and workflow logic, not in repeated JSON reads.

## 6. No Active `metadata.atm` Namespace

`metadata.atm` is not an approved namespace in the Phase U architecture.

Rules:

- ATM must not add or preserve machine-state fields under `metadata.atm`.
- ATM must not rely on `metadata.atm` reads for normal mailbox behavior.
- The active implementation must expose zero surviving `metadata.atm` fields.
- inbound shared-inbox records that still carry `metadata.atm` for backward
  compatibility are silently stripped, not rejected
