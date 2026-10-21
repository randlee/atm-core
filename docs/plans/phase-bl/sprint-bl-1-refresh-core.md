# Sprint bl-1 — collector contracts and persisted source state

`BOUNDARY-Collectors`; wave 1; no dependency. Create independent BD/dolt,
ATM, and GitHub collectors with short per-source locks and JSON records. It
also publishes the shared `StateSnapshot` and `EventRequest` data types used
by bl-3 and bl-5. Do not implement aggregate evaluation, debounce/delivery,
or message sending.

Each record contains value plus `last_success`, `last_attempt`, `last_error`,
and `since`. Default windows are BD/dolt 60 s, ATM 60 s, GitHub 600 s. Only
success gates, so a failed/partial read is eligible next request. Cron and TUI
use the same collector contract; when no read is due/lock is held they receive
and report persisted state without doubled source load. Own collector/state
code and deterministic injected-clock tests for independent rates, dedup,
retry-after-failure, and cron reporting TUI-produced data. Type fixtures prove
both consumers can construct an `EventRequest` without importing delivery
implementation.

## Relations, scope, and owned paths

`parallel_safe`: bl-2, bl-4, bl-8, bl-11. Consumers are bl-3 (types and
records), bl-5 (types), bl-6 (aggregate state), bl-7 (collector requests),
and bl-10 (record types). `must_follow`: none. Own new collector modules,
`state.py`, source adapters, codecs, `types.py`, CLI seams, and focused
collector/state tests. This sprint owns no aggregate-policy, display, event
debounce, or delivery files.

Owned filenames: `refresh.py`, `state.py`, `types.py`, `codec.py`, `cli.py`,
and source-adapter modules under `apps/atm-bd-console/src/atm_bd_console/`;
`tests/test_refresh.py`, `tests/test_state.py`, `tests/test_types.py`,
`tests/test_codec.py`, and `tests/test_cli.py` under the same package.

## Published contract

`CollectorRecord[T]` contains value plus `SourceStamp`; `StateSnapshot` is
the typed current aggregate; `EventRequest` names transition, recipients,
payload, created-at and debounce key. `request(source, by, clock)` returns a
persisted record when fresh or locked. `write_record` atomically replaces JSON.
No delivery implementation is imported by a consumer of these types.

## Governing text (verbatim)

> "my expectation for 'atm-bd cron' and 'atm-bd console' is that both are run on a specific phase. They both 'schedule' a info refresh which a) is exactly the same regardless of which system instigated it. b) is checked for duplication so running both does not DOUBLE the query load on bd/dolt, atm, github c) A failed run which did not produce successful data does NOT cause a run to be skipped."

> "we do NOT need to collect data from all sources at the same rate."

## Authoritative deliverables

1. Per-source lock/dedup and stamps; only `last_success` gates a read.
2. Independent 60/60/600-second eligibility and typed source errors.
3. JSON last-good records, state snapshots, and published event types.
4. Cron/TUI collector requests report the same persisted record without a
   duplicate source call.

## Deterministic acceptance and validation

- [ ] `test_collector_windows_are_source_specific`: a controlled clock proves BD/dolt and ATM are independently eligible at 60 seconds and GitHub at 600.
- [ ] `test_second_caller_within_window_reads_nothing`: a fake source records one call while both cron and TUI receive the persisted record.
- [ ] `test_refresh_lock_returns_persisted_in_progress_record`: a controlled held same-source lock never waits or duplicates a read.
- [ ] `test_failed_or_partial_read_is_due_on_next_request`: failure does not advance `last_success`; the next controlled request retries.
- [ ] `test_atomic_record_replacement_returns_last_good_snapshot`: injected storage proves a completed replacement exposes a whole record and its stamp.
- [ ] `test_public_types_construct_without_delivery_import`: a type-only fixture constructs `CollectorRecord`, `StateSnapshot`, and `EventRequest`.

```bash
cd apps/atm-bd-console && uv run --frozen ruff check src tests && uv run --frozen pyright src && uv run --frozen pytest -q
```

No test uses sleeps, threads, races, event-loop ordering, load, or a live service.

## Non-closure

Does not derive aggregate state, render it, create policy transitions, debounce requests, or deliver ATM messages. Those remain bl-6/bl-7 and bl-3 work.
