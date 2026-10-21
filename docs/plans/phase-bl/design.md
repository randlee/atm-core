# Phase bl design — collectors, state evaluation, and scheduled events

Phase bl uses independent collectors, a state evaluator, event requests, and
scheduled delivery. Persistence remains JSON state files; this design adds no
database.

## Governing text (verbatim)

> "collect-data (cron and/or tui) and save to db -> process data (determine state) and save to db -> fire events (after debouncing & aggregation)"

> "ATM messages occur on a specified schedule based on timing... nothing to do with either tui or cron."

> "we do NOT need to collect data from all sources at the same rate."

> "multiple collectors each in separate module governed by it's own rules (i.e. gh rate limited, dolt timeouts, atm no limits today). state evaluation based on current state. specific state transitions would trigger 'atm message request'."

The database wording describes architecture only: phase bl persists JSON files.

> "my expectation for 'atm-bd cron' and 'atm-bd console' is that both are run on a specific phase. They both 'schedule' a info refresh which a) is exactly the same regardless of which system instigated it. b) is checked for duplication so running both does not DOUBLE the query load on bd/dolt, atm, github c) A failed run which did not produce successful data does NOT cause a run to be skipped."

> "if you don't have valid data, don't force an error."

Tests use injected clocks and controlled fakes, never sleeps, races, threads,
event ordering, load, or live services.

## Current seam

`runner.step` currently collects, evaluates, sends, and persists in one call
(`runner.py:144-270`). `model.tick` evaluates and proposes actions
(`model.py:76-172`); `_run_actions` sends them (`runner.py:21-50`). Cron calls
that seam in `cli.py:60-79`; the TUI calls it on its interval in
`app.py:149-159,207-235`. The migration separates those concerns.

## Architecture

### Collectors

BD/dolt, ATM, and GitHub are separate modules with short-lived nonblocking
per-source locks and JSON records. Defaults are BD/dolt 60 s, ATM 60 s, and
GitHub 600 s; source-specific timeouts/rate limits remain source rules. Each
record has value plus `SourceStamp(last_success,last_attempt,last_error,since)`.
Only last_success gates collection, so failure or partial data is eligible on
the next request. The contract has force: r bypasses only due eligibility,
respects the per-collector lock, returns the in-progress/persisted record when a
lock is held, and never issues a second query. Success restarts that
collector's poll timer; failure stays due. Cron and TUI may request a
collector; fresh callers receive current persisted data and duplicate requests
do not double source load.

### State and display

After a collector writes, the state stage reads all current source records and
writes aggregate state/snapshot. It carries last-good content with typed
failure and age; never-read data is a labelled placeholder. `content_at` is
the oldest contributing good value. Rendering consumes only persisted state;
it neither schedules collection nor sends messages.

### Events and delivery

Transitions in aggregate state create durable event requests. The event stage
aggregates equivalent requests and debounces with injected time. A separate
ATM delivery schedule reads due requests, sends them, and records outcomes.
Collectors and TUI intervals never send messages; there is no requirement for
a fresh BD-and-ATM read before creating an event request.

### Recovery and tests

JSON replacements use temp-file + fsync + `os.replace`; delivery history is
fsynced. A crash before a success stamp leaves that collector eligible. Tests
independently prove source rate/lock/failure retry, evaluation from current
records, debounced request creation, scheduled delivery, and identical cron/TUI
views of persisted data.
