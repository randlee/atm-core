# Legacy ATM Message Schema

## 1. Ownership

This file documents the read-only compatibility schema for historical ATM-added
top-level inbox fields.

Ownership:

- ATM owns this compatibility contract.
- This file does not define new write behavior.
- New ATM-only message semantics must not be introduced as top-level fields in
  this legacy schema.

Enforcement model in this repo:

- `tools/schema_models/legacy_atm_message_schema.py`

## 2. Legacy ATM Top-Level Fields

Historical ATM-added top-level fields accepted on read:

- `message_id`
- `source_team`
- `pendingAckAt`
- `acknowledgedAt`
- `acknowledgesMessageId`

Observed historical ATM-only fields used for alerts and repair notices:

- `atmAlertKind`
- `missingConfigPath`

These fields are accepted for backward compatibility with historical inbox
data. They are not the forward schema contract for newly-authored ATM machine
state, which now lives in SQLite as documented in
[`atm-message-schema.md`](./atm-message-schema.md).

## 3. Read Compatibility Rule

ATM read and related workflows must continue to accept:

- Claude Code-native messages
- historical ATM messages using the legacy top-level additive fields documented
  here
- current ATM-authored compatibility messages documented in
  [`atm-message-schema.md`](./atm-message-schema.md)

## 4. Write Deprecation Rule

This schema is deprecated for write:

- ATM must not introduce new ATM-only top-level fields under this schema
- existing historical fields remain readable
- forward ATM machine state is documented as SQLite-owned in
  [`atm-message-schema.md`](./atm-message-schema.md)
- legacy top-level `atmAlertKind` and `missingConfigPath` remain read-compatible
  during the cleanup period and must not be removed from compatibility docs
  before the surviving read-compatibility paths are retired
