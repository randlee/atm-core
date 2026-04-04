# Claude Code Message Schema

## 1. Ownership

This file documents the Claude Code-native message schema that ATM consumes.

Ownership:

- Claude Code owns the native message schema and message-producing semantics.
- ATM must not redefine the Claude Code-native message schema.
- ATM may preserve unknown additive fields and may add ATM-authored fields only
  as documented in [`atm-message-schema.md`](./atm-message-schema.md).
- ATM must not use this file to justify introducing new ATM-only top-level
  fields into the shared inbox format.

Primary source used by this repo:

- `agent-team-mail/docs/agent-team-api.md`

Enforcement model in this repo:

- `tools/schema_models/claude_code_message_schema.py`

## 2. Native Inbox Message Shape

Claude Code-native baseline envelope used for native teammate delivery:

- `from`
- `text`
- `timestamp`
- `read`
- `summary`

Historically observed producer-owned optional field:

- `color`

Documented additive tolerance rule:

- absent fields should be treated as null
- unknown fields must be tolerated gracefully

## 3. Native Claude Code System Messages

The currently documented Claude Code idle notice is not a top-level inbox field
schema extension. It is JSON encoded inside the `text` field:

```json
{
  "type": "idle_notification",
  "from": "agent-name",
  "timestamp": "ISO 8601",
  "idleReason": "available"
}
```

Current ATM implication:

- ATM should treat this text-field JSON form as the canonical Claude Code idle
  notice format.
- ATM must not invent a replacement Claude-native idle schema.
- ATM may enrich a Claude-native message only by adding ATM-owned metadata as
  documented in [`atm-message-schema.md`](./atm-message-schema.md); it must not
  rewrite the native Claude fields to do so.

Validation rule:

- the Pydantic model for the native Claude Code message schema intentionally
  models only the Claude-owned fields and allows unknown additive fields, so
  ATM extensions do not become retroactively "native" by accident

## 4. What This File Does Not Define

This file does not define ATM-added persisted envelope fields such as:

- `message_id`
- `source_team`
- `pendingAckAt`
- `acknowledgedAt`
- `acknowledgesMessageId`
- ATM-specific alert metadata
- task object schema in `~/.claude/tasks/...`

`taskId` is intentionally not treated here as a Claude Code-native inbox
message field. ATM may interpret `taskId` when present, but that ownership is
documented in [`atm-message-schema.md`](./atm-message-schema.md), not here.

Historical provenance note:

- `quality-mgr` analysis over 7,297 persisted messages across 24 teams found
  the earliest Claude Code baseline messages using only
  `{from, text, timestamp, read, summary, color}`
- `message_id` first appeared later as an ATM-added field
- `source_team` appeared later still and always co-occurred with `message_id`
