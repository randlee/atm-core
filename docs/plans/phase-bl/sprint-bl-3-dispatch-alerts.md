# Sprint bl-3 — event requests, aggregation, and scheduled delivery

`BOUNDARY-EventRequests`; wave 2; must follow bl-1's `StateSnapshot` and
`EventRequest` types and bl-10's persisted problem-source records. Read those
current records, derive held/dispatch transitions, and debounce/aggregate
durable event requests. A separate schedule delivers due ATM requests and
records outcome. No collector/TUI sends messages and no fresh-BD-and-ATM rule
exists. Own dispatch/bead event code and deterministic record/debounce/delivery
tests; retained held rows include bead, task, reason, and source timestamp.
