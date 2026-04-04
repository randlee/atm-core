# Claude Code Message Schema

## 1. Ownership

This file documents the Claude Code-native message schema that ATM consumes.

Ownership:

- Claude Code owns the native message schema and message-producing semantics.
- ATM must not redefine the Claude Code-native message schema.
- ATM may preserve unknown additive fields and may add ATM-authored fields only
  as documented in [`atm-message-schema.md`](./atm-message-schema.md).

Primary source used by this repo:

- `agent-team-mail/docs/agent-team-api.md`

## 2. Native Inbox Message Shape

Documented inbox message fields observed by ATM:

- `from`
- `text`
- `timestamp`
- `read`
- `summary`

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

## 4. What This File Does Not Define

This file does not define:

- ATM-authored fields such as `message_id`, `pendingAckAt`,
  `acknowledgedAt`, `acknowledgesMessageId`, or `source_team`
- ATM-specific alert metadata
- task object schema in `~/.claude/tasks/...`

`taskId` is intentionally not treated here as a Claude Code-native inbox
message field. ATM may interpret `taskId` when present, but that ownership is
documented in [`atm-message-schema.md`](./atm-message-schema.md), not here.
