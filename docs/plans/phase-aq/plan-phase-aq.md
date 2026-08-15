---
title: Phase AQ Plan — Search Indexing and Query Optimization
status: AQ.1–AQ.2 planned
branch: plan/search-indexing-admission-performance
baseline: develop @ 5d541bd06
---

# Phase AQ Plan — Search Indexing and Query Optimization

## Why this phase exists

Phase AN delivered template-aware, host-local search with correct synchronous
FTS maintenance. That makes every successfully admitted canonical message
searchable immediately, but it also puts projection maintenance on the
foreground admission path. The M5 control results show the cost clearly:
the FTS-free control sustained 33,349 direct messages/second and 30,992 at
F8, while the develop-derived direct control sustained 16,603.

Phase AQ corrects that performance design without weakening mail correctness.
Canonical rows remain immediate durable truth. Search becomes an explicitly
eventually-consistent, durable projection: work is recorded atomically with
the source mutation and caught up by the existing SQLite writer only while
foreground admission is quiet.

This is a focused indexing/search-optimization phase. It does not reopen
Phase AN's template, workflow-metadata, query-language, or checked-render
work. AO (portable TLS) and AP (outbound corporate-network connectivity) are
independent planned phases and are not dependencies or deliverables of AQ.

**Phase AQ has no dependency on Phase AO or Phase AP, in either direction.**
AQ touches only `atm-storage-rusqlite`'s search-projection scheduling and the
sealed `atm-storage`/`atm-core` status DTO; it does not touch TLS transport
(AO) or outbound corporate-network connectivity (AP), and neither of those
phases touches search projection. AQ may be scheduled and executed before,
in parallel with, or after AO and/or AP, in any order, with no sequencing,
merge-order, or shared-artifact coordination required between them.

## Governing decision and boundaries

[ADR-047](../../adr/ADR-047-durable-idle-search-projection.md) is the
authoritative architecture decision. Its non-negotiable boundaries are:

- `atm-storage-rusqlite` exclusively owns the additive ledger, FTS state,
  coalescing, drain timing, and SQLite transactions.
- The existing private SQLite writer remains the single database owner. AQ
  introduces neither a second process nor a detached Tokio task.
- `atm-storage` owns only the sealed, backend-neutral
  `SearchProjectionStatus` read DTO/capability addition.
- `atm-core` maps that status without scheduler or SQLite knowledge; CLI and
  Tokio/Axum adapters present it only on local surfaces.
- The legacy synchronous daemon remains frozen and must not be changed.

## Delivery sequence

| Sprint | Doc | must_follow | Unblocks |
| --- | --- | --- | --- |
| AQ.1 durable idle search projection | [sprint-AQ1](./sprint-AQ1-durable-idle-search-projection.md) | Phase AN complete on `develop` | AQ.2 |
| AQ.2 search projection performance and recovery evidence | [sprint-AQ2](./sprint-AQ2-search-projection-performance-evidence.md) | AQ.1 pushed to `integrate/phase-aq` | Phase AQ release decision |

Every child merges its `must_follow` integration line before development or a
fix round. AQ.2 does not start a competing production implementation: it
validates AQ.1's exact candidate and may add only benchmark-harness fixes or
normal defect fixes discovered by the campaign.

## Entry and closure gates

### AQ.1 entry gate

The product owner must approve the additive SQLite desired-state work-ledger
schema before its migration is implemented. Record that approval in AQ.1's
implementation PR. The schema is a persistent behavior contract, not an
implementation detail to infer after coding starts.

### AQ.2 release gate

AQ.2 validates through the managed M5 benchmark procedure against the
retained FTS-free source control `3b67fea40`. It passes only when the AQ.1
candidate reaches both floors:

- direct: at least 30,014 messages/second;
- F8: at least 27,893 messages/second.

Those values are 90% of the retained same-hardware control. A lower result is
a regression, not a pass-by-absolute-rate. The report must additionally prove
durable recovery, explicit backlog observability, database isolation/restore,
and the final selected daemon pair. An actual correctness defect or confirmed
material regression requires rollback to the pre-benchmark pair; otherwise
the approved candidate remains running for dogfooding.

## Phase acceptance

Phase AQ closes only when all of the following are true:

1. Foreground admission atomically persists canonical source data plus one
   coalesced durable work item and performs no synchronous FTS catch-up.
2. The existing writer drains bounded work only while idle and yields to every
   foreground submission; restart, delete/recreate, and reindex converge to
   the current canonical state.
3. Local CLI, Python, HTTP, and doctor expose honest projection freshness
   without making canonical mail operations unavailable or adding remote
   search.
4. Required fake-contract, storage, core, CLI, local HTTP, Python, boundary,
   and cross-platform CI tests pass.
5. AQ.2 retains valid M5 direct/F8 and recovery evidence meeting the fixed
   performance gates.

## Explicit non-goals

- Remote search, a generic jobs framework, a user-selectable freshness
  policy, or a second database/indexer service.
- Changes to AO/AP's TLS or outbound-connectivity plans.
- SQLite/WAL tuning unrelated to the identified foreground FTS projection
  cost.
- ATM-owned workflow vocabulary, template approval/lineage enforcement, or
  raw SQL/FTS transport interfaces.
