# ATM Persisted Data Repair Guide

**Lifecycle**: Permanent cross-cutting document

This document provides operator guidance for persisted-data failures covered by:

- `REQ-P-CONFIG-HEALTH-001`
- `REQ-P-ERROR-001`
- `REQ-P-RELIABILITY-001`

It is intentionally not the source of truth for normative behavior. Product
requirements remain in [`requirements.md`](./requirements.md), and product
architecture remains in [`architecture.md`](./architecture.md).

## 1. Purpose

ATM reads persisted mailbox, team, and config data that may be:

- older than the current schema
- partially hand-edited
- externally generated
- truncated or corrupted by interrupted writes
- missing entirely

The goal is not "always continue." The goal is:

- continue when recovery is deterministic and low-risk
- isolate damage at the narrowest safe scope
- fail loudly when continuing would require guessing identity or routing data
- notify the operator or `team-lead` when degraded behavior needs repair

## 2. Recovery Ladder

ATM should treat team-config issues in this order:

1. Recover `compatibility-recoverable` forms with documented deterministic
   rules.
2. Isolate `record-invalid` entries when the surrounding document remains
   trustworthy.
3. Fail on `document-invalid` config with detailed repair guidance.
4. Treat `missing-document` separately from parse failure. Only command paths
   that explicitly support missing-document fallback may proceed.

All non-recoverable cases should report:

- failure class
- file path
- entity scope when known
- parser detail when available
- a safe repair action

## 3. Common Cases

### 3.1 Compatibility-Recoverable Member Form

Example:

```json
{
  "members": [
    "arch-ctm",
    { "name": "team-lead" }
  ]
}
```

Issue:
- the roster mixes a legacy string-member form with the canonical object form

Expected ATM behavior:
- accept both entries
- continue loading the team config
- avoid fabricating any extra identity or routing data

Safe repair:
- normalize the string entry to `{ "name": "arch-ctm" }`

Why this is recoverable:
- the member name is explicit and deterministic

### 3.2 Invalid Member In An Otherwise Valid Team

Example:

```json
{
  "members": [
    { "name": "arch-ctm" },
    { "broken": true },
    { "name": "team-lead" }
  ]
}
```

Issue:
- one member record is invalid, but the root document is still structurally
  trustworthy

Expected ATM behavior:
- isolate the invalid record
- continue serving valid members
- preserve a diagnostic that identifies the skipped member scope

Safe repair:
- repair the invalid entry or remove it

Why this is recoverable:
- the loader can identify the bad record without guessing hidden structure

### 3.3 Malformed Root JSON

Example:

```json
{
  "members": [
    { "name": "arch-ctm" }
```

Issue:
- truncated or syntactically invalid JSON document

Expected ATM behavior:
- fail the command
- report the file path and parser line/column when available
- avoid guessing missing structure

Safe repair:
- restore the file from a known-good copy or repair the JSON syntax

Why this is not recoverable:
- the document boundary itself is untrustworthy

### 3.4 Wrong Root Shape

Example:

```json
[
  { "name": "arch-ctm" }
]
```

Issue:
- the root value is an array when ATM expects a team config object

Expected ATM behavior:
- fail with a structured configuration error
- report that the root shape is invalid

Safe repair:
- wrap the member list in the expected object shape:
  `{ "members": [ ... ] }`

Why this is not recoverable:
- ATM cannot safely infer omitted root-level semantics

### 3.5 Wrong `members` Type

Example:

```json
{
  "members": {
    "name": "arch-ctm"
  }
}
```

Issue:
- `members` is an object instead of an array

Expected ATM behavior:
- fail with a structured configuration error
- include the field name in the repair guidance

Safe repair:
- repair `members` so it is an array of member records

Why this is not recoverable:
- the collection boundary is wrong, so record-level isolation is not safe

### 3.6 Missing Team Config During `send`

Example:

```text
~/.claude/teams/atm-dev/
  inboxes/
    recipient.json
```

Issue:
- `config.json` is missing entirely, but the recipient inbox already exists

Expected ATM behavior:
- treat this as `missing-document`, not as malformed JSON
- allow `send` to proceed only because the inbox path already exists
- surface an actionable warning to the sender
- send a best-effort repair notification to `team-lead` when that target can be
  resolved without guesswork
- deduplicate repeated repair notifications while the same missing-config
  condition remains unresolved

Safe repair:
- restore or recreate `config.json` for the team

Why this is only conditionally recoverable:
- delivery can proceed without guessing membership only because the inbox path
  already exists

### 3.7 Missing Team Config Without Safe Fallback

Example:

```text
~/.claude/teams/atm-dev/
  inboxes/
    team-lead.json
```

Issue:
- `config.json` is missing and the requested recipient inbox does not exist

Expected ATM behavior:
- fail the `send`
- explain that the missing config could not be bypassed safely
- tell the operator to restore team configuration or create the correct team
  state

Safe repair:
- restore `config.json` and retry, or create the intended team/inbox state by
  an approved workflow

Why this is not recoverable:
- creating or selecting a delivery target would require guesswork

## 4. Repair Principles

When repairing persisted data manually:

- prefer restoring known-good values over inventing new ones
- do not rename agents or teams unless the intent is explicit
- keep unknown fields unless the field is known to be corrupt
- validate JSON syntax before retrying ATM commands
- if the file may have been truncated, restore from backup before hand-editing
- when ATM used missing-config fallback, repair the team config promptly so
  future sends do not remain in degraded mode

## 5. Implementation Note

This document intentionally makes operator outcomes concrete so tests can be
written against the same cases:

- compatibility-recoverable forms should have positive tests
- isolated invalid-record handling should have scoped recovery tests
- non-recoverable document failures should assert file and parser context
- missing-config send fallback should assert sender warning, best-effort
  `team-lead` notification, and deduplication
