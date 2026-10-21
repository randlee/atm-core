# Sprint bl-3 — event requests, aggregation, and scheduled delivery

`BOUNDARY-EventRequests`; wave 2; must follow bl-1's source records. Read
current state, derive held/dispatch transitions, and write durable debounced,
aggregated event requests. A separate schedule delivers due ATM requests and
records outcome. No collector/TUI sends messages and no fresh-BD-and-ATM rule
exists. Own dispatch/bead event code and deterministic record/debounce/delivery
tests; retained held rows include bead, task, reason, and source timestamp.
