# Dedup And Metadata Schema Recommendation

## 1. Purpose

This document answers the design task for PR #18 idle-notification dedup,
missing-config alert dedup, and future metadata normalization.

Primary decision:

- schema ownership must be explicit before dedup rules are defined

Schema ownership files introduced with this design:

- [`../../claude-code-message-schema.md`](../../claude-code-message-schema.md)
- [`../../atm-message-schema.md`](../../atm-message-schema.md)
- [`../../legacy-atm-message-schema.md`](../../legacy-atm-message-schema.md)
- [`../../sc-observability-schema.md`](../../sc-observability-schema.md)

Enforcement models introduced with this design:

- `tools/schema_models/claude_code_message_schema.py`
- `tools/schema_models/atm_message_schema.py`
- `tools/schema_models/legacy_atm_message_schema.py`

## 2. Concrete Recommendations

### 2.1 Schema Ownership

Recommendation:

- Claude Code-native message schema is owned by Claude Code and documented in
  [`../../claude-code-message-schema.md`](../../claude-code-message-schema.md)
- ATM additive and interpreted fields are owned by ATM and documented in
  [`../../atm-message-schema.md`](../../atm-message-schema.md)
- `sc-observability` schema ownership is external and should be referenced via
  [`../../sc-observability-schema.md`](../../sc-observability-schema.md)

Ruling:

- ATM must not redefine Claude Code-native message schema
- ATM may only add additive fields and interpretation rules

### 2.2 Metadata Schema

Recommendation:

- `taskId` remains the only currently standardized ATM-interpreted shared field
- ATM-specific alert metadata remains ATM-prefixed additive fields
- legacy top-level ATM fields remain read-compatible but are not the forward
  write target
- do not standardize `priority`, `severity`, `error_code`, `repo`, `branch`,
  `ttl`, or `dedup_key` in the message schema in this sprint
- if ATM later normalizes ATM-owned machine fields into metadata, the
  ATM-defined `message_id` must use ULID for newly-authored values from day 1
  of that metadata schema
- the forward write target is `metadata.atm`, not additional top-level ATM-only
  fields

Concrete placement and ownership map:

- Claude-owned native fields remain top-level:
  - `from`
  - `text`
  - `timestamp`
  - `read`
  - `summary`
  - historically observed `color`
- shared/de facto field preserved at top-level when present:
  - `taskId`
- legacy ATM read-compatible top-level fields:
  - `message_id`
  - `source_team`
  - `pendingAckAt`
  - `acknowledgedAt`
  - `acknowledgesMessageId`
- forward ATM-owned machine metadata:
  - `metadata.atm.messageId`
  - `metadata.atm.sourceTeam`
  - `metadata.atm.fromIdentity`
  - `metadata.atm.pendingAckAt`
  - `metadata.atm.acknowledgedAt`
  - `metadata.atm.acknowledgesMessageId`
  - `metadata.atm.alertKind`
  - `metadata.atm.missingConfigPath`

Implementation guardrails:

- new ATM-only fields must not be introduced at top-level
- code and tests must reference the owning schema file when parsing or
  serializing message data

### 2.2.1 Write-Path Enforcement

Write-path rule:

- ATM-authored writes and explicit normalization/writer code paths may enforce
  strict ATM-owned identifier formats
- Pydantic validation or equivalent write-path schema enforcement must reject
  wrong-format ATM-owned identifiers with descriptive validation errors
- write-path rejection is acceptable because the writer is attempting to author
  or normalize an ATM-owned field, not merely preserve an incoming message

Write-path examples:

- legacy top-level `message_id` write compatibility accepts UUID and rejects
  ULID
- forward `metadata.atm.messageId` accepts ULID and rejects UUID
- forward `metadata.atm.acknowledgesMessageId` accepts the forward ATM message
  identity format for that schema revision and rejects other formats

### 2.2.2 Read-Path Enforcement

Read-path rule:

- read-path validation failure is not itself a message-read failure
- strict Pydantic validation may still be attempted first on the read path
- if read-path validation of ATM-owned identifier fields fails, ATM must log a
  format warning, treat the malformed ATM-owned field as absent for ATM
  semantics, and continue processing the message
- the message must not be dropped solely because an ATM-owned identifier field
  is malformed

Read-path examples:

- malformed legacy top-level `message_id` is preserved as raw stored data when
  possible, but ATM dedup/ack semantics treat it as absent
- malformed `metadata.atm.messageId` is preserved as raw stored data when
  possible, but ATM metadata semantics treat it as absent
- if the message otherwise satisfies the Claude-native schema, the message still
  appears in the read surface

Rationale:

- `taskId` already has documented ATM workflow semantics
- the other fields do not yet have a stable producer contract for inbox
  messages
- standardizing them now would risk ATM redefining external schema ownership
- `quality-mgr` provenance analysis confirms `message_id`, `source_team`,
  `pendingAckAt`, and `acknowledgesMessageId` are ATM-added fields rather than
  native Claude envelope fields

### 2.3 Idle Notification Classification

Recommendation:

- canonical current idle-notification detection remains the Claude Code-native
  JSON object in the `text` field with `type == "idle_notification"`
- ATM should use that native format as the dedup classification source
- ATM should not require extra idle metadata because the product goal is to
  keep only the newest idle notice and reduce clutter

### 2.4 Dedup Strategy

Recommendation:

- receiver-side dedup is the default for inbox-clutter controls
- sender-side dedup is reserved for ATM-authored back-channel repair notices
  that are intentionally best-effort and potentially repetitive
- do not add task-message dedup now, except a future exact-resend feature if
  explicitly designed later
- future metadata-based dedup should treat ATM `message_id` as an ATM-owned
  ULID identifier for new writes while preserving UUID read compatibility

Current dedup families:

- surface canonicalization dedup by `message_id`
- receiver-side idle-notification dedup by semantic message class
- sender-side ATM alert dedup by local ATM-owned alert state

### 2.5 Non-ATM Extension

Recommendation:

- ATM should preserve non-ATM additive fields
- ATM should allow future shared fields such as `repo` and `branch`
- ATM should not claim ownership of those fields until a shared schema is
  explicitly documented

### 2.6 Enrichment And Legacy Compatibility

Recommendation:

- ATM read must support Claude-native, legacy ATM top-level, and future
  metadata-based messages
- ATM may enrich a Claude-native message in place by adding `metadata.atm`
- cross-team alias projection may also record canonical sender identity in
  `metadata.atm.fromIdentity`
- enrichment must be additive and idempotent
- ATM-native inbox separation is explicitly deferred to a later version after
  the current shared inbox design is used live

Upgrade rule:

- a Claude-native message may be upgraded in place by adding `metadata.atm`
- the original Claude-owned fields remain authoritative for message content
- exception: a cross-team alias projection may retain a Claude-facing alias in
  `from` only when canonical sender identity is also recorded in
  `metadata.atm.fromIdentity`
- ATM workflow data such as ack state or ATM message identity attaches to that
  original stored message rather than moving the message into a different
  envelope format

### 2.7 Read-Path Degradation Rules

Read-path degradation contract:

- validation failure on the read path triggers recovery/degradation logic and
  observability warnings; it does not automatically fail the overall message
  read
- a validation pass may short-circuit recovery and warning logic because the
  message already conforms to the expected schema

#### 2.7.1 Claude-Native Fields Correct, ATM Fields Malformed

Required outcome:

- the message goes through
- malformed ATM-owned fields are treated as absent for ATM semantics
- observability emits a warning describing the field, expected format, and raw
  validation failure

Repair meaning:

- preserve the raw stored value when possible
- remove the malformed ATM-owned field from the interpreted ATM workflow view
- do not rewrite the Claude-native message content during read

#### 2.7.2 Claude System-Payload Interpretation Fails

Required outcome:

- the message goes through as a normal message when the underlying
  Claude-native envelope is still valid
- ATM-specific classification derived from that payload is treated as absent
- observability emits a warning that system-payload interpretation failed

Repair meaning:

- preserve the original `text`
- do not fabricate replacement idle/task/error metadata
- fall back to normal message handling when classification cannot be trusted

Example:

- if ATM attempts to parse `text` as a Claude idle-notification payload and the
  JSON parse fails, the message remains readable but is not treated as an idle
  notification

#### 2.7.3 Claude-Native Fields Unrecoverable

Minimum acceptable outcome:

- ATM must not invent missing Claude-native content
- ATM must surface an observability warning or error with file/message context
- ATM must preserve raw bytes in storage for diagnostics when preservation is
  possible
- ATM must not present that record as a normal usable message if the
  Claude-native envelope itself cannot be trusted

Repair meaning:

- no semantic repair of missing or unrecoverable Claude-native fields
- diagnostics and preservation are acceptable; fabrication is not

#### 2.7.4 Read-Mode Enforcement Structure

Implementation note:

- strict Pydantic models may remain the write-path enforcement mechanism
- read-path implementations may use the same strict models as a fast path, but
  must catch validation failures and route them into warning-producing
  degradation logic rather than treating them as hard read failures
- separate read-mode adapter classes or recovery helpers are acceptable if they
  preserve this contract

### 2.8 Rust Newtype Plan

Implementation requirement for J.1/J.2:

- Rust implementation must introduce distinct newtypes for the two ATM-owned
  message identifier families before the J.1/J.2 identifier-handling work is
  considered complete

Required newtypes:

- `AtmMessageId(Ulid)` for the forward ATM-owned metadata identifier
- `LegacyMessageId(Uuid)` for legacy top-level ATM identifier compatibility

Rationale:

- the design intentionally distinguishes forward ULID-based ATM identifiers
  from legacy UUID-based identifiers
- raw `String`, `Ulid`, or `Uuid` usage at every call site makes it too easy to
  assign one identifier family into the other by mistake
- Rust newtypes make the distinction compile-time visible and prevent accidental
  cross-assignment during J.1/J.2 implementation

Expected usage:

- write-path logic for `metadata.atm.messageId` should traffic in
  `AtmMessageId`
- legacy read-compatibility logic for top-level `message_id` should traffic in
  `LegacyMessageId`
- bridging code between the two schema eras must convert explicitly, never by
  implicit assignment

## 3. Dedup Taxonomy

### 3.1 Surface Canonicalization

Current implementation:

- `crates/atm-core/src/mailbox/surface.rs::dedupe_legacy_message_id_surface`
  is the single implementation used by read/ack/clear
- the key is the legacy ATM-authored top-level `message_id`
- collision handling keeps the newest message by timestamp; equal timestamps
  fall back to the later merged-surface position

Purpose:

- remove duplicate mailbox records from the operator-facing surface

Schema dependency:

- ATM-authored `message_id`

### 3.2 Receiver-Side Policy Dedup

Current or planned implementation:

- PR #18 idle-notification dedup keeps at most one unread idle notification per
  sender in an inbox

Purpose:

- reduce inbox clutter caused by recurring Claude Code idle notices

Schema dependency:

- Claude Code-native idle-notification JSON encoded in `text`

### 3.3 Sender-Side Policy Dedup

Current implementation on the hardening branch:

- missing-team-config repair notices are deduplicated before ATM emits repeated
  team-lead alerts for the same broken config path

Purpose:

- prevent repeated ATM-authored repair notices from flooding inboxes

Schema dependency:

- ATM-owned `atmAlertKind`
- ATM-owned `missingConfigPath`

See also:

- [`../../atm-message-schema.md`](../../atm-message-schema.md) §3 forward
  placement map
- [`../../atm-message-schema.md`](../../atm-message-schema.md) §5
  ATM-Specific Alert Metadata
## 4. Existing Implementation Review

### 4.1 PR #18: idle-notification-dedup

Conforms:

- idle-notification detection uses the currently documented Claude Code-native
  text-field JSON shape
- dedup goal is receiver-side clutter reduction, which matches the schema
  intent

Needs update:

- task-assignment classification should not be specified as a native Claude Code
  text-field schema until a producer contract exists
- the PR should reference the schema ownership files instead of implying ATM can
  extend Claude-native message schema ad hoc

### 4.2 PR #27 hardening branch

Conforms:

- missing-config notice dedup is correctly treated as ATM-authored behavior,
  not Claude-native message behavior
- additive fields live in the unknown-field map, so Claude-native schema is not
  replaced

Needs update:

- the ATM-owned alert metadata should be migrated from legacy top-level fields
  to `metadata.atm` in the forward schema
- future ATM alert fields should use explicit ATM-owned naming rather than
  unqualified shared names such as `error_code`
- see [`../../atm-message-schema.md`](../../atm-message-schema.md) §3 forward
  placement map and §5 ATM-Specific Alert Metadata for the J.4 alert-field
  migration specification

### 4.3 Current `atm-core` merge-surface dedup

Conforms:

- `message_id` dedup is an ATM-owned surface canonicalization rule
- provenance analysis confirms `message_id` itself is ATM-added, which makes
  this dedup family unambiguously ATM-owned

Resolved (J.3):

- sprint J.3 centralized legacy `message_id` surface canonicalization in
  `crates/atm-core/src/mailbox/surface.rs::dedupe_legacy_message_id_surface`
- the owning contract remains §3.1 Surface Canonicalization
- read-layer idle-notification collapse is now an explicit follow-on policy
  step on top of the shared surface canonicalization path rather than a second
  private message-id dedup implementation

## 5. Design Answers

### 5.1 Analyze PR #18

Answer:

- PR #18 is directionally correct on idle dedup because it follows the
  Claude Code-native idle-notification shape
- PR #18 is too loose on task-assignment classification because no native
  producer schema is established there yet

### 5.2 Deduplication Taxonomy

Answer:

- ATM has three dedup classes:
  - ATM `message_id` surface canonicalization
  - receiver-side semantic dedup for native repetitive notices
  - sender-side semantic dedup for ATM-authored repair notices

### 5.3 Metadata Schema

Answer:

- standardize only what is already stable:
  - ATM-authored fields
  - `taskId` as a shared/de facto interpreted field
- defer standardization of `priority`, `severity`, `error_code`, `repo`,
  `branch`, `ttl`, and `dedup_key`
- require ULID for ATM-defined `message_id` in the next metadata-based schema
  revision; legacy UUID remains read-compatible
- require ATM metadata to live under `metadata.atm` in the forward schema

### 5.4 Dedup Key Strategy

Answer:

- receiver-side by default
- sender-side only for ATM-authored repetitive repair notices
- no persisted generic `dedup_key` field in this sprint; keys are derived from
  stable schema fields per dedup class

### 5.5 Non-ATM Systems

Answer:

- preserve and pass through non-ATM fields
- allow future shared fields
- do not declare them part of the ATM schema until a shared owner explicitly
  defines them

## 6. Immediate Follow-On

Recommended follow-on doc cleanup after this design lands:

- reference the schema ownership files from top-level requirements and
  architecture docs
- keep the Pydantic enforcement models aligned with the schema ownership files
- remove or revise any task-assignment schema claims that exceed the documented
  producer contract
- align idle-notification fixtures that use top-level idle markers with the
  canonical Claude Code-native text-field JSON format where possible
- keep the deferred ATM-native inbox work out of the current live-schema branch
  until the shared inbox design has been exercised in production
