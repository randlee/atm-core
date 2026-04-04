# ATM Message Schema

## 1. Ownership

This file documents ATM-authored and ATM-interpreted additions layered on top
of the Claude Code-native message schema in
[`claude-code-message-schema.md`](./claude-code-message-schema.md).

Ownership:

- ATM owns only the additive fields and semantics defined in this file.
- ATM must preserve the native Claude Code schema and must not rename or
  replace it.
- ATM must tolerate unknown additive fields from other producers.

Enforcement model in this repo:

- `tools/schema_models/atm_message_schema.py`

## 2. ATM-Authored Additive Fields

Fields authored by ATM CLI messages and workflow mutations:

- `message_id`
- `pendingAckAt`
- `acknowledgedAt`
- `acknowledgesMessageId`
- `source_team`

Current semantics:

- `message_id` is ATM CLI only
- `pendingAckAt` and `acknowledgedAt` are ATM CLI workflow fields
- `acknowledgesMessageId` is an ATM reply-link field
- `source_team` is ATM routing metadata

These fields are additive and must coexist with the Claude Code-native message
shape without redefining it.

Historical provenance note:

- `quality-mgr` historical analysis identified `message_id`, `source_team`,
  `pendingAckAt`, and `acknowledgesMessageId` as ATM-added fields rather than
  Claude Code-native envelope fields

## 3. ATM-Interpreted Shared Or De Facto Fields

ATM currently interprets the following field when present:

- `taskId`

Ownership rule:

- ATM defines ATM workflow semantics for `taskId`
- ATM does not claim sole ownership of the field across all Claude-adjacent
  systems
- `taskId` must therefore be treated as a shared or de facto interoperable
  field, not as a Claude Code-native field and not as an ATM-exclusive field

Current ATM semantics for `taskId`:

- task-linked message
- acknowledgement required
- remains actionable until acknowledged
- must not be cleared before acknowledgement

Current evidence note:

- `taskId` is documented and interpreted by ATM, but it was not present in the
  current live `atm-dev` inbox data sampled during this design sprint

## 4. ATM-Specific Alert Metadata

The current missing-config fallback branch uses additive ATM-specific fields in
the unknown-field map:

- `atmAlertKind`
- `missingConfigPath`

These are ATM-owned fields.

Current design ruling:

- ATM-authored back-channel alerts may use ATM-prefixed additive fields
- new ATM-only fields should remain clearly ATM-owned until a broader shared
  schema is explicitly approved

Not standardized yet:

- `priority`
- `severity`
- `error_code`
- `repo`
- `branch`
- `ttl`
- `dedup_key`

These may be preserved if present, but ATM should not assign durable cross-team
message semantics to them until a dedicated schema decision is documented.
