# Storage Boundary Guidelines

This document defines stable storage-boundary rules.

## Storage rule

Storage backends must be replaceable.

`rusqlite` is an implementation detail, not a contract.

## Boundary rule

All storage interaction above the backend layer must go through storage traits
and storage-neutral types.

Concrete backend types must not appear above the backend crate.

## Dependency direction rule

Storage backends depend on neutral storage contracts.

They do not depend upward on facade, business, transport, or daemon
composition crates just to satisfy a trait or reuse convenience logic.

If a backend needs a contract and that contract lives in the wrong crate, the
contract must move.

## What to flag

Flag a boundary violation when:

- `rusqlite` types appear outside the backend crate
- backend code imports facade/business-layer logic
- a trait implemented by storage lives above storage and forces an upward
  dependency edge
- shared classifier or schema logic is duplicated because it was left in the
  wrong crate
- callers would need code changes if the backend were swapped

## What to prefer

- storage-neutral traits in the storage contract crate
- storage-neutral row/config/result types
- backend-local concrete connection handling
- moving contracts downward instead of adding dependency edges upward
- one canonical schema/classifier owner
