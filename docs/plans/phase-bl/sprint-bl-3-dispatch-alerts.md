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
sanity D5 selection, or TUI rendering. `must_follow`: bl-10's persisted problem-source records; bl-1's public record and request types. It is not parallel-safe with either producer, but is parallel-safe with unrelated wave-2 sprints once those artifacts are present.

## Governing text (verbatim)

> "ATM messages occur on a specified schedule based on timing... nothing to do with either tui or cron."

> "multiple collectors each in separate module governed by it's own rules (i.e. gh rate limited, dolt timeouts, atm no limits today). state evaluation based on current state. specific state transitions would trigger 'atm message request'."

## Public contract and deliverables

`enqueue(snapshot, transition, clock) -> EventRequest | None` derives typed
held, dispatch, and alert requests from persisted state. `due_requests(clock)`
returns aggregate/debounced requests; `deliver_due` records send outcome in
`DeliveryHistoryEntry(request_id, attempted_at, outcome)`. It performs no collection. Deliverables are transition
derivation, stable held rows, debounce/aggregate, and scheduled delivery.

## Deterministic acceptance and validation

- [ ] `test_held_row_preserves_bead_task_reason_and_source_stamp`: controlled persisted records derive the exact stable held-row fields.
- [ ] `test_dispatch_and_alert_transition_enqueue_typed_request`: each controlled transition yields the documented `EventRequest` shape.
- [ ] `test_equivalent_transitions_aggregate_by_debounce_key`: duplicate state transitions create one durable aggregate request.
- [ ] `test_delivery_not_due_until_injected_clock_advances`: advancing only the delivery clock makes a debounced request due.
- [ ] `test_delivery_history_records_success_and_failure`: fake ATM outcomes are persisted as delivery history without recollecting state.
- [ ] `test_collection_and_tui_never_send`: fakes prove collector/TUI paths enqueue only; the scheduled delivery seam alone invokes send.

```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q
```

All fixtures use controlled records and clocks; no live ATM calls, sleeps, races, or timing-sensitive tests are allowed.

## Non-closure

Does not define collector record types, collect sources, choose sanity D5, or render console rows. It consumes those producer artifacts and owns only transition-to-request, debounce, and scheduled delivery history.
