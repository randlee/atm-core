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
- `tools/schema_models/fixtures/claude_code_quality_mgr_samples.json`

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

Observed current `team-lead -> quality-mgr` sample classes used by this repo:

- plain Claude envelope
- Claude envelope plus producer/transport additive fields such as `metadata`
  or `type`

Those additive fields do not become Claude-owned native schema merely because
they are tolerated by the read path.

Current ATM compatibility note:

- healthy current Claude `.json` inbox files are one top-level JSON array of
  inbox-message objects
- healthy current Claude `.json` inbox files are the primary shared inbox
  path ATM must support directly
- repair/rebuild is reserved for malformed or truly unsupported inbox content,
  not for the legal current Claude JSON-array shape

## 2.1 Current Shared Inbox Contract Categories

This repository distinguishes three categories at the shared inbox boundary:

1. Claude Code-native envelope
   - `from`
   - `text`
   - `timestamp`
   - `read`
   - optional `summary`
   - optional producer-owned `color`
2. tolerated unknown additive fields on that envelope
   - producer/transport additions such as `metadata` or `type`
   - these must not fail reads, but they are not promoted into the Claude-owned
     native contract
3. historical ATM-owned additive fields
   - documented separately in [`atm-message-schema.md`](./atm-message-schema.md)
   - not part of the current Claude-owned contract
   - queued for narrower compatibility handling in `AA.10`

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
  rewrite the native Claude fields to do so except for the explicitly
  documented ATM-owned cross-team alias projection carve-out on `from`.

Validation rule:

- the Pydantic model for the native Claude Code message schema intentionally
  models only the Claude-owned fields and allows unknown additive fields, so
  ATM extensions do not become retroactively "native" by accident

## 3.1 ATM Compatibility Projection Note

When ATM re-exports one of its own oversized messages into Claude Code JSONL,
ATM may replace the Claude-visible `text` field with exactly:

- `atm read --message-id <id>`

In that stub:

- `<id>` is the shared inbox `message_id` that ATM and Claude-compatible
  consumers already use on the compatibility surface

That replacement is an ATM-owned compatibility projection rule, not a
Claude-owned schema change. The durable full body remains ATM-owned state and
the governing policy lives in
[`ADR-010`](./adr/ADR-010-claude-jsonl-compatibility-envelope.md).

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

This file also does not define ATM-authored JSONL export size policy, retrieval
stub behavior, or durable-store limits. Those ATM-owned compatibility-envelope
rules are documented in
[`ADR-010`](./adr/ADR-010-claude-jsonl-compatibility-envelope.md).

Historical provenance note:

- `quality-mgr` analysis over 7,297 persisted messages across 24 teams found
  the earliest Claude Code baseline messages using only
  `{from, text, timestamp, read, summary, color}`
- `message_id` first appeared later as an ATM-added field
- `source_team` appeared later still and always co-occurred with `message_id`
- current redacted `team-lead -> quality-mgr` fixture samples also show that
  `metadata` and `type` may appear as tolerated additive fields while the
  Claude-owned envelope remains the same
