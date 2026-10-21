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
