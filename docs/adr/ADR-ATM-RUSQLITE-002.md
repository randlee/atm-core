# ADR-ATM-RUSQLITE-002 — Single In-Process SQLite Write Worker

```yaml
adr_id: ADR-ATM-RUSQLITE-002
crate: atm-storage-rusqlite
title: Single in-process SQLite write worker
status: accepted
related_adrs:
  - ADR-047 # extends this writer's private scheduling with durable idle search projection
date: 2026-05-10
decided-date: 2026-05-10
deciders:
  - team-lead
  - arch-ctm
tags:
  - sqlite
  - concurrency
  - batching
related_boundaries:
  - BOUNDARY-MailStore-Sqlite
  - BOUNDARY-RosterStore-Sqlite
code_references:
  - crates/atm-storage-rusqlite/src/shared_db.rs
  - crates/atm-storage-rusqlite/src/lib.rs
```

## Context

`atm-rusqlite` currently opens a fresh production SQLite connection per write
operation and relies on SQLite’s internal single-writer lock to serialize
concurrent mutations. The mailbox append path is especially hot and still pays
for a pre-write `SELECT` probe plus one commit per logical write.

SQLite WAL supports concurrent readers, but still permits only one writer at a
time. The application is therefore already serialized at the storage engine’s
write boundary, just without explicit batching or write-ownership control.

## Decision

Introduce one crate-private in-process SQLite write worker that:
- owns one long-lived write connection
- accepts bounded typed write submissions from `SharedDb`
- drains queued writes in bounded batches
- preserves the current `atm-core` store trait contracts
- remains private to `atm-rusqlite`

This ADR intentionally keeps the worker private because the application
already pays SQLite's single-writer serialization cost; the change is to make
that ownership explicit, bounded, and locally optimizable without widening
public store contracts. Batching is part of the crate's internal write policy,
not a new cross-crate abstraction.

This ADR does not approve SQLite task persistence as a product source of
truth. Any current `TaskStore` code in `atm-rusqlite` is outside the approved
storage-architecture baseline and is not normative for later storage-contract
work.

Phase `AC` follow-up note:
- `ADR-018` supersedes any historical `atm-core` store-trait references here
  when the backend converges into `atm-storage-rusqlite`

## Consequences

- write-path serialization becomes explicit at the crate boundary
- the hot mailbox append path can remove its pre-write probe and use
  row-count-based insertion detection when message-row immutability holds
- reader concurrency remains available through separate read handles
- worker health and shutdown semantics become a first-class design concern

## Alternatives Considered

- keep ad-hoc per-operation write connections and rely only on SQLite lock
  contention handling
- add Tokio-based blocking task orchestration inside `atm-rusqlite`
- widen public store contracts around a generic public writer abstraction

## Follow-Up

- benchmark the resulting hot-path throughput and latency
- review WAL autocheckpoint tuning separately if sustained write load requires
  it
