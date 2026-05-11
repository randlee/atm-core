# ADR-ATM-RUSQLITE-002 — Single In-Process SQLite Write Worker

```yaml
adr_id: ADR-ATM-RUSQLITE-002
crate: atm-rusqlite
title: Single in-process SQLite write worker
status: accepted
date: 2026-05-10
deciders:
  - team-lead
  - arch-ctm
tags:
  - sqlite
  - concurrency
  - batching
related_boundaries:
  - BOUNDARY-MailStore-Sqlite
  - BOUNDARY-TaskStore-Sqlite
  - BOUNDARY-RosterStore-Sqlite
code_references:
  - crates/atm-rusqlite/src/shared_db.rs
  - crates/atm-rusqlite/src/lib.rs
  - crates/atm-rusqlite/src/writer/mod.rs
  - crates/atm-rusqlite/src/writer/ops.rs
  - crates/atm-rusqlite/src/writer/stmt_cache.rs
  - docs/phase-S/sprint-S15-rusqlite-plan.md
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
- migrates the Phase T hot write path first:
  - `MailStore::upsert_message`
  - `MailStore::upsert_visibility_state`
- leaves surviving cold-path `SharedDb::with_transaction(...)` callers in place
  only when the T.2 caller audit explicitly records them as non-hot-path
  writes

## Consequences

- write-path serialization becomes explicit at the crate boundary
- the connection budget becomes explicit as `1` permanent writer handle plus up
  to `3` transient reader handles
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

- use `docs/phase-S/sprint-S15-rusqlite-plan.md` as the canonical design for
  the S.15 implementation shape and any follow-on QA reconciliation
- the `T.3` immutable-row addendum completes the hot mailbox contract by:
  - removing the pre-write message existence probe
  - making `mail_messages` insert-only with duplicate-key `DO NOTHING`
  - rejecting crate-owned logical invariants before SQL submission where the
    crate already owns the rule
- benchmark the resulting hot-path throughput and latency
- review WAL autocheckpoint tuning separately if sustained write load requires
  it
