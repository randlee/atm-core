# Dedup And Metadata Schema Recommendation

## 1. Purpose

This document answers the design task for PR #18 idle-notification dedup,
missing-config alert dedup, and future metadata normalization.

Primary decision:

- schema ownership must be explicit before dedup rules are defined

Schema ownership files introduced with this design:

- [`../../claude-code-message-schema.md`](../../claude-code-message-schema.md)
- [`../../atm-message-schema.md`](../../atm-message-schema.md)
- [`../../sc-observability-schema.md`](../../sc-observability-schema.md)

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
- do not standardize `priority`, `severity`, `error_code`, `repo`, `branch`,
  `ttl`, or `dedup_key` in the message schema in this sprint

Rationale:

- `taskId` already has documented ATM workflow semantics
- the other fields do not yet have a stable producer contract for inbox
  messages
- standardizing them now would risk ATM redefining external schema ownership

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

## 3. Dedup Taxonomy

### 3.1 Surface Canonicalization

Current implementation:

- read/ack/clear deduplicate merged surfaces by `message_id` with last-wins
  behavior

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

- the ATM-owned alert metadata should be documented in
  [`../../atm-message-schema.md`](../../atm-message-schema.md)
- future ATM alert fields should use explicit ATM-owned naming rather than
  unqualified shared names such as `error_code`

### 4.3 Current `atm-core` merge-surface dedup

Conforms:

- `message_id` dedup is an ATM-owned surface canonicalization rule

Needs update:

- the duplicated `dedupe_sourced_messages` helpers in read/ack/clear should be
  centralized later to keep the dedup contract consistent

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
- remove or revise any task-assignment schema claims that exceed the documented
  producer contract
- align idle-notification fixtures that use top-level idle markers with the
  canonical Claude Code-native text-field JSON format where possible
