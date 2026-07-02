# Phase AC Type Convergence Map

## Goal

Identify the semantic record families that must converge into canonical shared
types and the current duplication patterns that caused the current boundary
volume.

This is an `AC.0` planning collateral artifact used by `AC.1` and `AC.5`.

## Canonical Families To Converge

The shared contract is expected to converge around a small set of semantic
families:

- `Message`
- `MessageKey`
- `MessageQuery`
- `RosterMember`
- `RosterSnapshot`
- notification event types

## Current Duplicate Families

### Message Family

Current message-shaped duplication appears across:

- `MessageEnvelope` and logical delivery message layers
- `MailStoreMessageRecord`
- mailbox metadata rows
- RPC request / response envelopes
- Claude mailbox file representations

Planning direction:

- one canonical `Message` record for storage and RPC body use
- operation/query wrappers only where semantics truly differ

### Roster Family

Current roster-shaped duplication appears across:

- `RosterMemberRecord`
- `ProjectionRosterMember`
- `ProjectionRoster`
- `RosterStore*Request` / `RosterStore*Response`
- config ingress / doctor / projection surfaces

Planning direction:

- one canonical roster member shape
- one canonical roster snapshot shape
- Claude-specific projection details remain backend behavior, not shared types

### Task Family

Current task-shaped duplication appears across:

- `TaskStoreTaskRecord`
- `TaskStoreTaskMetadata`
- `TaskStore*Request` / `TaskStore*Response`
- task-link and ack-transition wrapper types

Planning direction:

- task storage is deferred out of the initial Phase `AC` shared contract
- these shapes are not canonical seeds for `atm-storage`
- any future task-storage line must start from canonical Claude-code schema
  plus Pydantic validation, not from these speculative wrappers

## RPC Convergence Rule

RPC must carry:

- one generic envelope
- canonical body structs

It must not preserve:

- per-message transport clones
- a separate storage-shaped message family
- wrapper proliferation just because a backend persists only part of a record

## Required Use In Later Sprints

- `AC.1` uses this map to define the canonical shared message/roster type set
  in `atm-storage`
- `AC.5` uses this map to collapse RPC body duplication for those approved
  shared families
- `AC.6` uses this map to verify redundant type families were deleted
