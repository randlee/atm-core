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

## 2. Current ATM-Authored Additive Fields

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

Forward migration requirement:

- if ATM moves its machine-readable fields into metadata rather than top-level
  additive envelope fields, the ATM-defined `message_id` format must be ULID
  from day 1 of that metadata schema
- ATM read compatibility must continue to accept legacy UUID `message_id`
  values and missing `message_id` values in historical inbox data

Deprecation rule:

- the fields in this section are legacy top-level ATM write behavior
- they remain read-compatible, but new ATM-only semantics must not continue to
  proliferate as new top-level additive fields

Legacy top-level placement map:

- legacy `message_id` is ATM-owned, top-level, and read-compatible only
- legacy `source_team` is ATM-owned, top-level, and read-compatible only
- legacy `pendingAckAt` is ATM-owned, top-level, and read-compatible only
- legacy `acknowledgedAt` is ATM-owned, top-level, and read-compatible only
- legacy `acknowledgesMessageId` is ATM-owned, top-level, and read-compatible
  only

## 3. Forward ATM Metadata Schema

Forward write target for ATM-owned machine-readable fields:

- top-level `metadata` object
- ATM-owned namespace under `metadata.atm`

`metadata.atm` fields planned for ATM-authored or ATM-enriched messages:

- `messageId`
- `sourceTeam`
- `fromIdentity`
- `pendingAckAt`
- `acknowledgedAt`
- `acknowledgesMessageId`
- ATM-owned alert metadata such as `alertKind`
- ATM-owned alert metadata such as `missingConfigPath`

Ownership rule:

- ATM owns `metadata.atm`
- shared or producer-defined fields may coexist in `metadata`, but ATM must not
  assume ownership of the full `metadata` object

Forward-write requirements:

- no new ATM-only top-level fields
- ATM machine-readable data moves to `metadata.atm`
- `messageId` must be ULID for newly-authored ATM metadata records
- ATM must generate `messageId` first and derive the persisted Claude-native
  `timestamp` from the ULID time component so both values represent the same
  creation instant
- legacy top-level ATM fields remain read-compatible only

Enrichment rule:

- ATM may upgrade a Claude-native message by adding `metadata.atm` to the
  original stored message
- enrichment must be additive and idempotent
- ATM must not rewrite native Claude fields such as `from`, `text`,
  `timestamp`, `read`, or `summary` in order to attach ATM metadata
- exception: when cross-team alias projection is intentionally used, ATM may
  retain the Claude-facing alias in `from` only when canonical sender identity
  is also recorded in `metadata.atm.fromIdentity`

Forward placement map:

- legacy top-level `message_id` migrates to `metadata.atm.messageId`
- legacy top-level `source_team` migrates to `metadata.atm.sourceTeam`
- cross-team alias projection stores canonical sender identity in
  `metadata.atm.fromIdentity`
- legacy top-level `pendingAckAt` remains `metadata.atm.pendingAckAt`
- legacy top-level `acknowledgedAt` remains `metadata.atm.acknowledgedAt`
- legacy top-level `acknowledgesMessageId` remains
  `metadata.atm.acknowledgesMessageId`
- legacy ATM alert fields such as `atmAlertKind` migrate to
  `metadata.atm.alertKind`
- legacy top-level `missingConfigPath` migrates to
  `metadata.atm.missingConfigPath`

Identifier rules:

- legacy top-level `message_id` remains UUID-based read compatibility
- forward `metadata.atm.messageId` must be ULID
- forward `metadata.atm.acknowledgesMessageId` must reference the ULID-based
  ATM message identity for the acknowledged message
- for ATM-authored forward records, ATM generates the ULID first and derives
  the persisted Claude-native `timestamp` from that ULID creation time
- when present, `metadata.atm.messageId` is also the primary workflow-sidecar
  key for `.claude/teams/<team>/.atm-state/workflow/<agent>.json`
- write-path enforcement may reject wrong-format ATM-owned identifiers for the
  active schema revision
- read-path validation failure for wrong-format ATM-owned identifiers must warn,
  preserve the message when the Claude-native envelope is still usable, and
  treat the malformed ATM-owned field as absent for ATM semantics

## 4. ATM-Interpreted Shared Or De Facto Fields

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

## 5. ATM-Specific Alert Metadata

The current missing-config fallback branch uses ATM-specific fields in the
legacy top-level schema:

- `atmAlertKind`
- `missingConfigPath`

These are ATM-owned fields.

Current design ruling:

- ATM-authored back-channel alerts may use ATM-prefixed fields during the
  legacy compatibility period
- forward ATM alert metadata should move under `metadata.atm`
- `atmAlertKind` migrates to `metadata.atm.alertKind`
- `missingConfigPath` migrates to `metadata.atm.missingConfigPath`
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
