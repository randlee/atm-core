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

The goal is not "always continue." The goal is:

- continue when recovery is deterministic and low-risk
- isolate damage at the narrowest safe scope
- fail loudly when continuing would require guessing identity or routing data

## 2. Recovery Ladder

ATM should treat persisted-data issues in this order:

1. Recover with a documented default when the missing data is compatibility-only
   and the fallback does not change identity or routing semantics.
2. Isolate or skip one invalid record when the surrounding collection is still
   trustworthy.
3. Fail the command when the root document is malformed or when recovery would
   require guessing identity, membership, or routing data.

All non-recoverable cases should report:

- file path
- entity scope when known
- field name when known
- parser detail, including line and column when available
- a safe repair action

## 3. Common Cases

### 3.1 Missing Compatibility Field

Example:

```json
{
  "agentId": "arch-ctm@atm-dev",
  "name": "arch-ctm",
  "model": "gpt5.3-codex",
  "joinedAt": 1770765919076,
  "cwd": "/workspace"
}
```

Issue:
- legacy `AgentMember` record omits `agentType`

Expected ATM behavior:
- recover with the documented default
- continue loading the team config
- preserve a warning or diagnostic that compatibility recovery was used

Safe repair:
- add `"agentType": "general-purpose"` to the member record

Why this is recoverable:
- `agentType` is descriptive capability metadata
- defaulting it does not invent membership or routing identity

### 3.2 Unknown Future Field

Example:

```json
{
  "agentId": "arch-ctm@atm-dev",
  "name": "arch-ctm",
  "agentType": "general-purpose",
  "model": "gpt5.3-codex",
  "joinedAt": 1770765919076,
  "cwd": "/workspace",
  "futureFeature": { "enabled": true }
}
```

Issue:
- newer writer added a field older ATM versions do not understand

Expected ATM behavior:
- preserve the field during round-trip when possible
- continue without warning unless the field affects a feature this binary owns

Safe repair:
- none required

Why this is recoverable:
- the data is structurally valid and forward-compatible

### 3.3 Missing Identity Or Routing Field

Example:

```json
{
  "name": "arch-ctm",
  "agentType": "general-purpose",
  "model": "gpt5.3-codex",
  "joinedAt": 1770765919076,
  "cwd": "/workspace"
}
```

Issue:
- required identity field such as `agentId` is missing

Expected ATM behavior:
- do not invent the missing value
- isolate the invalid member only if the surrounding roster can still be used
- otherwise fail with a precise config error

Safe repair:
- restore the missing `agentId` field using the canonical `agent@team` value

Why this is not auto-recoverable:
- fabricating `agentId` would guess membership and routing semantics

### 3.4 Wrong Type In A Member Record

Example:

```json
{
  "agentId": "arch-ctm@atm-dev",
  "name": "arch-ctm",
  "agentType": "general-purpose",
  "model": "gpt5.3-codex",
  "joinedAt": "1770765919076",
  "cwd": "/workspace"
}
```

Issue:
- field type does not match the schema

Expected ATM behavior:
- do not silently coerce the value unless the product contract explicitly
  allows that coercion
- isolate the member if the remaining collection is trustworthy
- include the failing field and parser detail in the diagnostic

Safe repair:
- change `"joinedAt"` to a JSON number, not a string

Why this is usually not auto-recoverable:
- generic coercions hide malformed data and make future corruption harder to
  detect

### 3.5 Malformed Root JSON

Example:

```json
{
  "teamName": "atm-dev",
  "members": [
    { "agentId": "arch-ctm@atm-dev", "name": "arch-ctm" }
```

Issue:
- truncated or syntactically invalid JSON document

Expected ATM behavior:
- fail the command
- report the file path and parser line/column
- avoid guessing what the missing structure should have been

Safe repair:
- restore the file from a known-good copy or repair the JSON structure

Why this is not recoverable:
- the document boundary itself is untrustworthy

### 3.6 Wrong Root Shape

Example:

```json
[
  {
    "agentId": "arch-ctm@atm-dev",
    "name": "arch-ctm",
    "agentType": "general-purpose"
  }
]
```

Issue:
- root shape is an array when ATM expects a team config object

Expected ATM behavior:
- fail the command with a structured configuration error
- report that the root shape is invalid rather than attempting schema
  migration by guesswork

Safe repair:
- wrap the member list in the expected team config object shape

Why this is not recoverable:
- ATM cannot safely infer omitted root-level fields or semantics

### 3.7 One Invalid Member In An Otherwise Valid Team

Example:

```json
{
  "teamName": "atm-dev",
  "members": [
    {
      "agentId": "arch-ctm@atm-dev",
      "name": "arch-ctm",
      "agentType": "general-purpose",
      "model": "gpt5.3-codex",
      "joinedAt": 1770765919076,
      "cwd": "/workspace"
    },
    {
      "name": "broken-member"
    }
  ]
}
```

Issue:
- one record is invalid inside an otherwise valid roster

Expected ATM behavior:
- isolate the invalid member if the loader can still trust the containing
  collection
- continue serving unaffected members
- fail commands that specifically target the invalid member

Safe repair:
- repair or remove the invalid member entry

Why this is only conditionally recoverable:
- collection-level recovery is safe only when the loader can identify a single
  bad record without guessing hidden structure

## 4. Repair Principles

When repairing persisted data manually:

- prefer restoring known-good values over inventing new ones
- do not rename agents or teams unless the intent is explicit
- keep unknown fields unless the field is known to be corrupt
- validate JSON syntax before retrying ATM commands
- if the file may have been truncated, restore from backup before hand-editing

## 5. Implementation Note

This document intentionally makes operator outcomes concrete so tests can be
written against the same cases:

- compatibility-only schema drift should have positive tests
- isolated invalid-record handling should have scoped recovery tests
- non-recoverable document failures should have diagnostic tests that assert
  file and parser context
