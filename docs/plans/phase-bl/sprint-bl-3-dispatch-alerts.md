# Sprint bl-3 — event requests, aggregation, and scheduled delivery

`BOUNDARY-EventRequests`; wave 2; uses bl-1's `StateSnapshot` and
`EventRequest` types and must follow bl-10's persisted problem-source records.
Read those current records, derive held/dispatch transitions, and debounce/aggregate
durable event requests. A separate schedule delivers due ATM requests and
records outcome. No collector/TUI sends messages and no fresh-BD-and-ATM rule
exists. Own dispatch/bead event code and deterministic record/debounce/delivery
tests; retained held rows include bead, task, reason, and source timestamp.

## Scope and owned paths

Own `dispatch.py`, `beads_source.py`, event-request/delivery JSON state and
their tests. It does not own collectors, `EventRequest` type definitions,
sanity D5 selection, or TUI rendering.

## Public contract and deliverables

`enqueue(snapshot, transition, clock) -> EventRequest | None` derives typed
held, dispatch, and alert requests from persisted state. `due_requests(clock)`
returns aggregate/debounced requests; `deliver_due` records send outcome in
delivery history. It performs no collection. Deliverables are transition
derivation, stable held rows, debounce/aggregate, and scheduled delivery.

## Deterministic acceptance and validation

- Controlled persisted records prove held/dispatch/alert event shapes.
- Repeated equivalent transitions aggregate; controlled clock advancement
  proves debounce and delivery; no collector or TUI call sends.
- Run frozen ruff, pyright, and pytest; no sleeps or live ATM calls.
