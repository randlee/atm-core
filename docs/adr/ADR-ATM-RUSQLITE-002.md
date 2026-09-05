# ADR-ATM-RUSQLITE-002 — Single In-Process SQLite Write Worker

```yaml
adr_id: ADR-ATM-RUSQLITE-002
crate: atm-rusqlite
title: Single in-process SQLite write worker
status: accepted
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
  - crates/atm-rusqlite/src/shared_db.rs
  - crates/atm-rusqlite/src/lib.rs
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

## Phase AW amendment (2026-09)

The single writer now owns a second, lower-priority diagnostic timeline lane.
Its channel carries batches of at most 128 events and has eight batch slots;
therefore best-effort diagnostics have a hard in-flight ceiling of 1,024
events. The worker uses a biased receive: primary durable-state work is always
selected first, and it drains no more than one diagnostic batch on an idle
tick after the primary channel is empty.

Diagnostic producers use `try_send` only. A full or unavailable diagnostic
lane drops the whole batch and records counters; a persistence failure is also
counted and dropped. Neither case changes a mailbox, acknowledgement, send,
or read outcome, and no diagnostic path opens a competing SQLite writer
connection. Retention pruning runs on that same lower-priority worker after a
bounded written-row interval; every individual deletion statement is capped
at 1,000 rows.
