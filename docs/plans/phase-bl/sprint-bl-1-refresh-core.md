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
and bl-10 (record types). Own new collector modules, `state.py`, source
adapters, codecs, `types.py`, CLI seams, and focused collector/state tests.

## Published contract

`CollectorRecord[T]` contains value plus `SourceStamp`; `StateSnapshot` is
the typed current aggregate; `EventRequest` names transition, recipients,
payload, created-at and debounce key. `request(source, by, clock)` returns a
persisted record when fresh or locked. `write_record` atomically replaces JSON.
No delivery implementation is imported by a consumer of these types.

## Authoritative deliverables

1. Per-source lock/dedup and stamps; only `last_success` gates a read.
2. Independent 60/60/600-second eligibility and typed source errors.
3. JSON last-good records, state snapshots, and published event types.
4. Cron/TUI collector requests report the same persisted record without a
   duplicate source call.

## Deterministic acceptance and validation

- Injected clocks/fakes prove each rate, same-source lock contention,
  failure-retry, partial failure, atomic persisted return, and type-only
  construction of `EventRequest`.
- No sleeps, threads, races, load, or real services occur in tests.
- Run the console package's frozen ruff, pyright, and pytest commands.
