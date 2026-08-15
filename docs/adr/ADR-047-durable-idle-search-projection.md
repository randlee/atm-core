# ADR-047 — Durable Idle Search Projection

| Field | Value |
| --- | --- |
| Status | Proposed for AN.16–AN.17 planning |
| Scope | `atm-storage`, `atm-storage-rusqlite`, local search freshness and maintenance observability |
| Relates to | ADR-001, ADR-008, ADR-018, ADR-036, ADR-ATM-RUSQLITE-002; `REQ-P-SEARCH-INDEX-001`, `REQ-RUSQLITE-SEARCH-INDEX-001` |

## Context

AN.5 made the SQLite FTS external-content projections transactionally exact by
calling `sync_message_projection` after each new mailbox row and after each
decomposed admission update. The operation reads the canonical row, writes the
external-content row, and lets SQLite's FTS triggers update the virtual table
before the foreground admission transaction replies.

That correctness choice is measurable admission-path work. On the M5 direct
SQLite control, the pre-AN.5 FTS-free source (`3b67fea40`) sustained 33,349
messages/second direct and 30,992 messages/second at F8. The current
develop-derived direct control sustained 16,603 messages/second: a real
throughput regression, but no evidence of data loss or a functional defect.

Search is a derived, recoverable local projection. Canonical mailbox and
template rows are the durable source of truth; a stale FTS row must never
alter mail admission, routing, acknowledgement, or canonical reads.

## Decision

### 1. Coalesce durable work with the source mutation

`atm-storage-rusqlite` will own an additive, private SQLite work ledger for
message and template search projections. Source-row creation, decomposed
admission update, and deletion enqueue one idempotent desired-state work item
in the *same writer transaction* as their canonical mutation. The ledger is
bounded by distinct source identities rather than write volume: repeated
mutations of one source coalesce to one current work item.

The ledger records no user-controlled query syntax and no new workflow
vocabulary. A message item's stable identity is `(team, agent, message_key)`;
a template item's identity is `template_sha`. During drain, the worker reads
the current canonical row. If it exists, it upserts the matching external
content projection; if it no longer exists, it removes the projection. Only
after that projection operation commits does it remove the durable work item.
The operation is crash-safe and idempotent: a crash leaves a work item to
replay, never a falsely-complete item.

The normal admission path does **not** synchronously catch up FTS merely to
return a successful send. It performs only the canonical mutation and the
cheap idempotent work-ledger enqueue.

The additive migration preserves the already-valid AN.5 FTS rows and starts
with an empty ledger; it must not rebuild the entire projection during normal
daemon startup. Explicit `reindex-search` is the recovery/backfill operation.

### 2. The existing SQLite writer owns idle maintenance

There is no second daemon, background database owner, detached Tokio task, or
HTTP-runtime database access. The existing private single SQLite writer from
ADR-ATM-RUSQLITE-002 owns maintenance on its connection. After a configured
quiet interval with no foreground writer submission, it drains one bounded
work batch in its own transaction. It yields immediately whenever foreground
work arrives; foreground admission always wins the next batch.

The worker's startup and shutdown remain owned by `SharedDb` / the concrete
SQLite store. It records a bounded shutdown outcome and resumes unfinished
ledger rows after reopen. It must not issue maintenance work while shutting
down or retain a detached thread/task after the store is dropped.

### 3. Make staleness explicit, never silently authoritative

The existing `MessageSearchStore` is extended with a small backend-neutral,
read-only `SearchProjectionStatus` DTO and its existing Tokio-safe async
companion gets the equivalent deadline-bounded status call. It reports at
least pending work count, oldest pending timestamp (if any), last successful
drain timestamp, and the latest completed projection watermark. The SQLite
adapter owns its calculation through the existing bounded reader lane; fakes
expose deterministic values.

Local CLI/HTTP/Python search results and doctor diagnostics surface this
status as an additive observation. A nonempty backlog is not a daemon
readiness failure and does not make a canonical message unavailable, but it
must make it clear that FTS results are an eventually-consistent projection.
No remote search endpoint is added. `atm admin reindex-search` remains the
explicit recovery operation and uses the same desired-state/drain semantics,
not a competing projection writer.

### 4. One honest performance gate

The implementation must be measured through the managed M5 benchmark process
against the historical same-host control, never by a reduced throughput
threshold. The initial acceptance floor is 90% of the retained FTS-free M5
baseline: at least 30,014 direct messages/second and 27,893 F8
messages/second, using the same profile and isolation procedure. A run below
either floor is a release blocker until it has a documented root cause and a
new explicitly-approved baseline; it cannot be called passing merely because
it exceeds a small absolute number.

## Boundaries

| Layer | Owns | Must not own |
| --- | --- | --- |
| `atm-storage` | `SearchProjectionStatus` and sync/async read-only status methods on the existing sealed search capability | SQLite work tables, worker timing, FTS commands, HTTP/CLI formatting |
| `atm-storage-rusqlite` | schema, transactional desired-state ledger, idle drain, coalescing, and private metrics | workflow/business vocabulary, public query syntax, separate runtime service |
| `atm-core` | transport-neutral mapping of the status accompanying a local search result | scheduling, SQLite details, FTS catch-up |
| CLI / `atm-http-runtime` | additive presentation/codec of core status | direct storage access, worker construction, policy that blocks sends on search backlog |

This extends the existing sealed `MessageSearchStore` capability, not a new
catch-all maintenance trait. Any new implementation or fake follows ADR-001
and the storage boundary manifests before code lands.

## Rejected alternatives

1. **Synchronous FTS maintenance on every admission.** Correct but measured
   as the hot-path regression this decision remedies.
2. **In-memory queue or best-effort Tokio task.** Loses work across a crash
   and creates a second database owner/lifecycle.
3. **A separate indexer process or HTTP endpoint.** Adds operational topology
   and violates the one in-process SQLite ownership model.
4. **Make search synchronously flush before every query.** Moves the same
   unbounded work to reads and hides staleness/latency from users.
5. **Treat FTS as canonical truth.** Invalid: canonical tables remain the
   durable mail/template truth and can rebuild the projection.

## Required evidence

- Transactional tests for enqueue-with-source-mutation, coalescing, crash
  replay, delete/recreate ordering, restart recovery, and no foreground FTS
  synchronization.
- Deterministic idle/fairness tests proving foreground writes preempt
  maintenance and drain batches are bounded.
- Fake-contract, core, CLI, local HTTP, and Python observations proving
  backlog status is explicit and search remains local-only.
- Fresh managed M5 direct/F8 benchmark evidence satisfying the approved
  floor, plus a recovery/reindex equivalence check and retained artifacts.
