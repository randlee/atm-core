# Phase Y Shared-Inbox Field Inventory

Purpose:

- enumerate every ATM-authored compatibility inbox field
- record the current keep/remove/undecided disposition
- define the decision framework Sprint `Y.5` must use before any field
  survives the Phase `Y` line

Decision rule:

- `keep` only if the field is immutable correlation or context that SQLite
  alone cannot provide to the shared inbox consumer at read time
- `remove` if the field reflects mutable workflow truth that belongs in SQLite
- `undecided` only when a concrete consumer still needs audit or replacement

## Inventory

| Field | Current role | Disposition | Why |
|---|---|---|---|
| `message_id` | stable ATM/SQLite correlation id | `keep` | required to correlate shared-inbox records to durable SQLite records |
| `parentMessageId` | immutable thread/update lineage pointer | `keep` | required to correlate thread/update records without reconstructing lineage heuristically |
| `threadMode` | immutable thread/update mode (`add-details`, `supersede`, etc.) | `keep` | part of the authored message semantics, not mutable workflow state |
| `taskId` | immutable task correlation id when present | `keep` | task linkage is correlation context, not mutable workflow truth |
| `source_team` | sender/source routing context | `undecided` | keep only if a real shared-inbox consumer still needs it after Y.5 audit |
| `pendingAckAt` | mutable ack workflow state | `remove` | belongs in SQLite workflow truth |
| `acknowledgedAt` | mutable ack workflow state | `remove` | belongs in SQLite workflow truth |
| `acknowledgesMessageId` | reply/ack linkage emitted by ATM | `undecided` | may be retained as immutable correlation, but only if a concrete consumer still needs it |
| `metadata.atm.*` mutable workflow projections | legacy mutable ATM state carrier | `remove` | Phase Y target is to stop projecting mutable workflow truth into compatibility output |

## Decision Framework For Y.5

Sprint `Y.5` must evaluate each field with this checklist:

1. Is the field immutable after message creation?
2. Is the field needed for correlation or authored message semantics?
3. Can the same truth be recovered from SQLite at read time instead?
4. Does a real retained consumer depend on the field today?
5. If the field is removed, should the dependent logic be deleted instead of
   preserved?

Disposition rules:

- `keep` requires:
  - immutable semantics
  - explicit consumer justification
  - written statement for why SQLite-only lookup is insufficient
- `remove` is the default when the field carries mutable workflow truth
- `undecided` is temporary and must be resolved during `Y.5`; it is not a
  release-ready final state

## Y.5 Expected Outcome

- only immutable ATM-authored correlation/context fields survive
- mutable workflow truth remains SQLite-only
- hidden consumers exposed by removal are deleted or refactored before `Y.6`
